use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
};

const BSIZE: usize = 1024; // Size in bytes of a single filesystem block.
const FSSIZE: u32 = 2000; // size of file system in blocks.
const NINODES: u32 = 200; // Total number of inodes available in the filesystem.
const FSMAGIC: u32 = 0x10240390; // Magic number used to identify and validate this filesystem format.
const LOGBLOCKS: u32 = 30; // Number of log blocks reserved for the filesystem journal/log.
const NDIRECT: usize = 12; // Number of direct data block addresses stored in each inode.
const DIRSIZ: usize = 14; // Maximum number of bytes in a directory entry name.
const ROOTINO: u32 = 1; // Inode number of the root directory.
const T_DIR: u16 = 1; // Inode type value for a directory.
const T_FILE: u16 = 2; // Inode type value for a regular file.
const NINDIRECT: usize = BSIZE / 4; // Number of block addresses that can fit in one indirect block.

const MAXFILE: usize = NDIRECT + NINDIRECT; // Maximum number of data blocks a file can reference (direct blocks + indirect blocks).
const IPB: u32 = (BSIZE as u32) / (std::mem::size_of::<Dinode>() as u32); // Number of inodes that fit in a single filesystem block.
const BPB: u32 = (BSIZE as u32) * 8; // Number of bits in one bitmap block, i.e. how many data blocks a single bitmap block can track.

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Superblock {
    magic: u32,
    size: u32,
    nblocks: u32,
    ninodes: u32,
    nlog: u32,
    logstart: u32,
    inodestart: u32,
    bmapstart: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Dinode {
    r#type: u16,
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Dirent {
    inum: u16,
    name: [u8; DIRSIZ],
}

impl Default for Dirent {
    fn default() -> Self {
        Self {
            inum: 0,
            name: [0; DIRSIZ],
        }
    }
}

struct Mkfs {
    fsfd: File,
    sb: Superblock,
    freeinode: u32,
    freeblock: u32,
}

impl Mkfs {
    fn wsect(&mut self, sec: u32, buf: &[u8]) -> io::Result<()> {
        assert_eq!(buf.len(), BSIZE);
        self.fsfd.seek(SeekFrom::Start(sec as u64 * BSIZE as u64))?;
        self.fsfd.write_all(buf)?;
        Ok(())
    }

    fn rsect(&mut self, sec: u32, buf: &mut [u8]) -> io::Result<()> {
        assert_eq!(buf.len(), BSIZE);
        self.fsfd.seek(SeekFrom::Start(sec as u64 * BSIZE as u64))?;
        self.fsfd.read_exact(buf)?;
        Ok(())
    }

    fn winode(&mut self, inum: u32, inode: &Dinode) -> io::Result<()> {
        let bn = iblock(inum, &self.sb);
        let mut buf = [0u8; BSIZE];
        self.rsect(bn, &mut buf)?;

        let inode_size = std::mem::size_of::<Dinode>();
        let offset = (inum as usize % IPB as usize) * inode_size;
        let raw = serialize_dinode(inode);
        buf[offset..offset + inode_size].copy_from_slice(&raw);

        self.wsect(bn, &buf)
    }

    fn balloc(&mut self, used: u32) -> io::Result<()> {
        let mut buf = [0u8; BSIZE];
        for i in 0..used {
            buf[(i / 8) as usize] |= 1 << (i % 8);
        }
        self.wsect(self.sb.bmapstart, &buf)
    }

    fn iappend(&mut self, inum: u32, data: &[u8]) -> io::Result<()> {
        let mut din = self.rinode(inum)?;
        let mut off = u32::from_le(din.size);
        let mut p = 0usize;

        while p < data.len() {
            let fbn = (off as usize) / BSIZE;
            assert!(fbn < MAXFILE);

            let x = if fbn < NDIRECT {
                if u32::from_le(din.addrs[fbn]) == 0 {
                    din.addrs[fbn] = xint(self.freeblock);
                    self.freeblock += 1;
                }
                u32::from_le(din.addrs[fbn])
            } else {
                if u32::from_le(din.addrs[NDIRECT]) == 0 {
                    din.addrs[NDIRECT] = xint(self.freeblock);
                    self.freeblock += 1;
                }

                let indirect_block = u32::from_le(din.addrs[NDIRECT]);
                let mut indirect_buf = [0u8; BSIZE];
                self.rsect(indirect_block, &mut indirect_buf)?;

                let idx = fbn - NDIRECT;
                let start = idx * 4;
                let mut entry = u32::from_le_bytes([
                    indirect_buf[start],
                    indirect_buf[start + 1],
                    indirect_buf[start + 2],
                    indirect_buf[start + 3],
                ]);

                if entry == 0 {
                    entry = self.freeblock;
                    self.freeblock += 1;
                    indirect_buf[start..start + 4].copy_from_slice(&entry.to_le_bytes());
                    self.wsect(indirect_block, &indirect_buf)?;
                }

                entry
            };

            let n1 = std::cmp::min(data.len() - p, (fbn + 1) * BSIZE - off as usize);

            let mut buf = [0u8; BSIZE];
            self.rsect(x, &mut buf)?;
            let start = off as usize - fbn * BSIZE;
            buf[start..start + n1].copy_from_slice(&data[p..p + n1]);
            self.wsect(x, &buf)?;

            p += n1;
            off += n1 as u32;
        }

        din.size = xint(off);
        self.winode(inum, &din)
    }

    fn rinode(&mut self, inum: u32) -> io::Result<Dinode> {
        let bn = iblock(inum, &self.sb);
        let mut buf = [0u8; BSIZE];
        self.rsect(bn, &mut buf)?;

        let inode_size = std::mem::size_of::<Dinode>();
        let offset = (inum as usize % IPB as usize) * inode_size;
        Ok(deserialize_dinode(&buf[offset..offset + inode_size]))
    }

    fn ialloc(&mut self, typ: u16) -> io::Result<u32> {
        let inum = self.freeinode;
        self.freeinode += 1;

        let din = Dinode {
            r#type: xshort(typ),
            major: 0,
            minor: 0,
            nlink: xshort(1),
            size: xint(0),
            addrs: [0; NDIRECT + 1],
        };

        self.winode(inum, &din)?;
        Ok(inum)
    }
}

fn xshort(x: u16) -> u16 {
    x.to_le()
}

fn xint(x: u32) -> u32 {
    x.to_le()
}

fn serialize_superblock(sb: &Superblock) -> [u8; BSIZE] {
    let mut buf = [0u8; BSIZE];
    let fields = [
        sb.magic,
        sb.size,
        sb.nblocks,
        sb.ninodes,
        sb.nlog,
        sb.logstart,
        sb.inodestart,
        sb.bmapstart,
    ];

    for (i, v) in fields.iter().enumerate() {
        buf[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }

    buf
}

fn serialize_dinode(d: &Dinode) -> Vec<u8> {
    let mut out = Vec::with_capacity(std::mem::size_of::<Dinode>());
    out.extend_from_slice(&d.r#type.to_le_bytes());
    out.extend_from_slice(&d.major.to_le_bytes());
    out.extend_from_slice(&d.minor.to_le_bytes());
    out.extend_from_slice(&d.nlink.to_le_bytes());
    out.extend_from_slice(&d.size.to_le_bytes());
    for a in &d.addrs {
        out.extend_from_slice(&a.to_le_bytes());
    }
    out
}

fn deserialize_dinode(buf: &[u8]) -> Dinode {
    let mut off = 0;
    let read_u16 = |buf: &[u8], off: &mut usize| {
        let v = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
        *off += 2;
        v
    };
    let read_u32 = |buf: &[u8], off: &mut usize| {
        let v = u32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
        *off += 4;
        v
    };

    let r#type = read_u16(buf, &mut off);
    let major = read_u16(buf, &mut off);
    let minor = read_u16(buf, &mut off);
    let nlink = read_u16(buf, &mut off);
    let size = read_u32(buf, &mut off);

    let mut addrs = [0u32; NDIRECT + 1];
    for a in &mut addrs {
        *a = read_u32(buf, &mut off);
    }

    Dinode {
        r#type,
        major,
        minor,
        nlink,
        size,
        addrs,
    }
}

#[inline]
fn iblock(inum: u32, sb: &Superblock) -> u32 {
    inum / IPB + sb.inodestart
}

fn serialize_dirent(de: &Dirent) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[0..2].copy_from_slice(&de.inum.to_le_bytes());
    buf[2..2 + DIRSIZ].copy_from_slice(&de.name);
    buf
}

fn make_dirent(inum: u16, name: &str) -> Dirent {
    let mut de = Dirent::default();
    de.inum = xshort(inum);
    let bytes = name.as_bytes();
    de.name[..bytes.len()].copy_from_slice(bytes);
    de
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: mkfs fs.img files...");
        std::process::exit(1);
    }

    let image_path = &args[1];
    let files = &args[2..];

    let fsfd = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(image_path)?;

    let nbitmap = FSSIZE / BPB + 1;
    let ninodeblocks = NINODES / IPB + 1;
    let nlog = LOGBLOCKS + 1;
    let nmeta = 2 + nlog + ninodeblocks + nbitmap;
    let nblocks = FSSIZE - nmeta;

    let sb = Superblock {
        magic: xint(FSMAGIC),
        size: xint(FSSIZE),
        nblocks: xint(nblocks),
        ninodes: xint(NINODES),
        nlog: xint(nlog),
        logstart: xint(2),
        inodestart: xint(2 + nlog),
        bmapstart: xint(2 + nlog + ninodeblocks),
    };

    let mut mkfs = Mkfs {
        fsfd,
        sb,
        freeinode: 1,
        freeblock: nmeta,
    };

    let zeroes = [0u8; BSIZE];
    for i in 0..FSSIZE {
        mkfs.wsect(i, &zeroes)?;
    }

    let sb_block = serialize_superblock(&mkfs.sb);
    mkfs.wsect(1, &sb_block)?;

    let rootino = mkfs.ialloc(T_DIR)?;
    assert_eq!(rootino, ROOTINO);

    let dot = serialize_dirent(&make_dirent(rootino as u16, "."));
    mkfs.iappend(rootino, &dot)?;

    let dotdot = serialize_dirent(&make_dirent(rootino as u16, ".."));
    mkfs.iappend(rootino, &dotdot)?;

    for path in files {
        let mut shortname = path.as_str();
        if let Some(stripped) = shortname.strip_prefix("user/") {
            shortname = stripped;
        }

        assert!(!shortname.contains('/'));

        if let Some(stripped) = shortname.strip_prefix('_') {
            shortname = stripped;
        }

        assert!(shortname.len() <= DIRSIZ);

        let inum = mkfs.ialloc(T_FILE)?;
        let de = serialize_dirent(&make_dirent(inum as u16, shortname));
        mkfs.iappend(rootino, &de)?;

        let mut hostf = std::fs::File::open(path)?;
        let mut buf = [0u8; BSIZE];
        loop {
            let n = hostf.read(&mut buf)?;
            if n == 0 {
                break;
            }
            mkfs.iappend(inum, &buf[..n])?;
        }
    }

    let mut din = mkfs.rinode(rootino)?;
    let mut off = u32::from_le(din.size);
    off = ((off / BSIZE as u32) + 1) * BSIZE as u32;
    din.size = xint(off);
    mkfs.winode(rootino, &din)?;

    mkfs.balloc(mkfs.freeblock)?;
    Ok(())
}
