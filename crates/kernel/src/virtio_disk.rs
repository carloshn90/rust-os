use core::ptr::null_mut;

use crate::memory::{PAGE_SIZE, k_alloc};

pub const VIRTIO0: usize = 0x0a000000;
const VIRTIO_MMIO_MAGIC_ADDRESS: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
const VIRTIO_MMIO_STATUS: usize = 0x070;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
const VIRTIO_MMIO_DRIVER_DESC_LOW: usize = 0x090;
const VIRTIO_MMIO_DRIVER_DESC_HIGH: usize = 0x094;
const VIRTIO_MMIO_DEVICE_DESC_LOW: usize = 0x0a0;
const VIRTIO_MMIO_DEVICE_DESC_HIGH: usize = 0x0a4;

const VIRTIO_CONFIG_S_ACKNOWLEDGE: u32 = 1;
const VIRTIO_CONFIG_S_DRIVER: u32 = 2;
const VIRTIO_CONFIG_S_FEATURES_OK: u32 = 8;
const VIRTIO_CONFIG_S_DRIVER_OK: u32 = 4;

// device feature bits
const VIRTIO_BLK_F_RO: u32 = 5; // Disk is read-only
const VIRTIO_BLK_F_SCSI: u32 = 7; // Supports scsi command passthru
const VIRTIO_BLK_F_CONFIG_WCE: u32 = 11; // Writeback mode available in config
const VIRTIO_F_ANY_LAYOUT: u32 = 27;
const VIRTIO_RING_F_INDIRECT_DESC: u32 = 28;
const VIRTIO_RING_F_EVENT_IDX: u32 = 29;

const NUMBER_OF_SLOTS: usize = 32;
const NUMBER_OF_QUEUES: usize = 8;
const BLOCK_SIZE: usize = 1024;

#[repr(C)]
pub struct Buf {
    pub valid: i32,
    pub disk: i32,
    pub dev: u32,
    pub blockno: u32,
    pub refcnt: u32,
    pub prev: *mut Buf,
    pub next: *mut Buf,
    pub data: [u8; BLOCK_SIZE],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Info {
    b: *mut Buf,
    status: u8,
}

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,                    // always zero
    idx: u16,                      // driver will write ring[idx] next
    ring: [u16; NUMBER_OF_QUEUES], // descriptor numbers of chain heads
    unused: u16,
}

#[repr(C)]
struct virtqUsedElem {
    id: u32, // index of start of completed descriptor chain
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16, // always zero
    idx: u16,   // device increments when it adds a ring[] entry
    ring: [virtqUsedElem; NUMBER_OF_QUEUES],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct VirtioBlkReq {
    kind: u32, // VIRTIO_BLK_T_IN or ..._OUT
    reserved: u32,
    sector: u64,
}

#[repr(C)]
struct Disk {
    pub desc: *mut VirtqDesc,
    pub avail: *mut VirtqAvail,
    pub used: *mut VirtqUsed,
    pub free: [u8; NUMBER_OF_QUEUES],
    pub used_idx: u16,
    pub info: [Info; NUMBER_OF_QUEUES],
    pub ops: [VirtioBlkReq; NUMBER_OF_QUEUES],
}

static mut DISK: Disk = Disk {
    desc: null_mut(),
    avail: null_mut(),
    used: null_mut(),
    free: [0; NUMBER_OF_QUEUES],
    used_idx: 0,
    info: [Info {
        b: null_mut(),
        status: 0,
    }; NUMBER_OF_QUEUES],
    ops: [VirtioBlkReq {
        kind: 0,
        reserved: 0,
        sector: 0,
    }; NUMBER_OF_QUEUES],
};

#[inline]
fn mmio_read(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn mmio_write(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

pub fn virtio_disk_init() {
    let slot: usize = (0..NUMBER_OF_SLOTS)
        .find(|&i| {
            let base = VIRTIO0 + i * 0x200;
            mmio_read(base + VIRTIO_MMIO_DEVICE_ID) == 2
        })
        .expect("Could not find virtio disk");

    let base = VIRTIO0 + slot * 0x200;
    let magic = mmio_read(base + VIRTIO_MMIO_MAGIC_ADDRESS);
    let version = mmio_read(base + VIRTIO_MMIO_VERSION);
    let device_id = mmio_read(base + VIRTIO_MMIO_DEVICE_ID);
    let vendor_id = mmio_read(base + VIRTIO_MMIO_VENDOR_ID);

    if magic != 0x74726976 || version != 1 || device_id != 2 || vendor_id != 0x554d4551 {
        panic!("device found but validation failed");
    }

    // reset device
    let mut status = 0;
    mmio_write(base + VIRTIO_MMIO_STATUS, status);

    // set Acknowledge status bit
    status |= VIRTIO_CONFIG_S_ACKNOWLEDGE;
    mmio_write(base + VIRTIO_MMIO_STATUS, status);

    // set Driver status bit
    status |= VIRTIO_CONFIG_S_DRIVER;
    mmio_write(base + VIRTIO_MMIO_STATUS, status);

    // negotiate features
    let mut features: u64 = mmio_read(base + VIRTIO_MMIO_DEVICE_FEATURES) as u64;
    features &= !(1u64 << VIRTIO_BLK_F_RO);
    features &= !(1u64 << VIRTIO_BLK_F_SCSI);
    features &= !(1u64 << VIRTIO_BLK_F_CONFIG_WCE);
    features &= !(1u64 << VIRTIO_F_ANY_LAYOUT);
    features &= !(1u64 << VIRTIO_RING_F_EVENT_IDX);
    features &= !(1u64 << VIRTIO_RING_F_INDIRECT_DESC);
    mmio_write(base + VIRTIO_MMIO_DRIVER_FEATURES, features as u32);

    // tell device that feature negotiation is complete
    status |= VIRTIO_CONFIG_S_FEATURES_OK;
    mmio_write(base + VIRTIO_MMIO_STATUS, status);

    // re-read status to ensure FEATURES_OK is set.
    status = mmio_read(base + VIRTIO_MMIO_STATUS);
    if (status & VIRTIO_CONFIG_S_FEATURES_OK) == 0 {
        panic!("virtio disk FEATURES_OK unset");
    }

    // initialize queue 0.
    mmio_write(base + VIRTIO_MMIO_QUEUE_SEL, 0);

    // ensure queue 0 is not in use.
    if mmio_read(base + VIRTIO_MMIO_QUEUE_READY) != 0 {
        panic!("virtio disk should not be ready");
    }

    // Check maximum queue size
    let max = mmio_read(base + VIRTIO_MMIO_QUEUE_NUM_MAX);
    if max == 0 {
        panic!("virtio disk has no queue 0")
    }
    if max < (NUMBER_OF_QUEUES as u32) {
        panic!("virtio disk max queue too short")
    }

    // set queue size
    mmio_write(base + VIRTIO_MMIO_QUEUE_NUM, NUMBER_OF_QUEUES as u32);

    unsafe {
        // allocate and zero queue memory
        let desc = k_alloc() as *mut VirtqDesc;
        let avail = k_alloc() as *mut VirtqAvail;
        let used = k_alloc() as *mut VirtqUsed;

        if desc.is_null() || avail.is_null() || used.is_null() {
            panic!("virtio disk kalloc");
        }
        core::ptr::write_bytes(desc as *mut u8, 0, PAGE_SIZE);
        core::ptr::write_bytes(avail as *mut u8, 0, PAGE_SIZE);
        core::ptr::write_bytes(used as *mut u8, 0, PAGE_SIZE);

        // write physical addresses
        let desc_addr = desc as u64;
        mmio_write(base + VIRTIO_MMIO_QUEUE_DESC_LOW, desc_addr as u32);
        mmio_write(base + VIRTIO_MMIO_QUEUE_DESC_HIGH, (desc_addr >> 32) as u32);

        let avail_addr = avail as u64;
        mmio_write(base + VIRTIO_MMIO_DRIVER_DESC_LOW, avail_addr as u32);
        mmio_write(
            base + VIRTIO_MMIO_DRIVER_DESC_HIGH,
            (avail_addr >> 32) as u32,
        );

        let used_addr = used as u64;
        mmio_write(base + VIRTIO_MMIO_DEVICE_DESC_LOW, used_addr as u32);
        mmio_write(
            base + VIRTIO_MMIO_DEVICE_DESC_HIGH,
            (used_addr >> 32) as u32,
        );

        DISK.desc = desc;
        DISK.avail = avail;
        DISK.used = used;

        // all NUMBER_OF_QUEUES start out unused
        for i in 0..NUMBER_OF_QUEUES {
            DISK.free[i] = 1;
        }
    }

    // queue is ready
    mmio_write(base + VIRTIO_MMIO_QUEUE_READY, 0x1);

    // tell device we're completely ready.
    status |= VIRTIO_CONFIG_S_DRIVER_OK;

    mmio_write(base + VIRTIO_MMIO_STATUS, status);
}
