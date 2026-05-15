use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use hal::klog;

use crate::{
    memory::{PAGE_SIZE, RAM_END, k_alloc, pg_round_up},
    process::{NUMBER_OF_PROCESS, Process},
    uart_logger::{UART0_BASE, logger},
    virtio_disk::VIRTIO0,
};

type Pte = u64;

// One beyond the highest lower-half virtual address we support.
// We use one bit less than the full 48-bit VA space to avoid
// dealing with canonical addresses with bit 47 set.
const MAX_VA: u64 = 1 << (9 + 9 + 9 + 9 + 12 - 1);

const DESC_VALID: u64 = 1 << 0;
const DESC_TABLE: u64 = 1 << 1;
const DESC_PAGE: u64 = 1 << 1;
const AF: u64 = 1 << 10;
const ATTRIDX0: u64 = 0 << 2;
const ATTRIDX1: u64 = 1 << 2;

const AP_EL1_RW_EL0_NONE: u64 = 0b00 << 6;
#[allow(dead_code)]
const AP_EL1_RW_EL0_RW: u64 = 0b01 << 6;
const AP_EL1_RO_EL0_NONE: u64 = 0b10 << 6;
#[allow(dead_code)]
const AP_EL1_RO_EL0_RO: u64 = 0b11 << 6;

const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;

const KERNBASE: u64 = 0x40080000;
const TRAMPOLINE: u64 = MAX_VA - PAGE_SIZE as u64;
const TRAP_FRAME: u64 = TRAMPOLINE - PAGE_SIZE as u64;
const GICD_BASE: u64 = 0x0800_0000;
const GICC_BASE: u64 = 0x0801_0000;
const GICD_SIZE: u64 = 0x10000;
const GICC_SIZE: u64 = 0x10000;

unsafe extern "C" {
    static etext: u8;
}
unsafe extern "C" {
    fn enable_mmu(ttbr0: u64);
}

unsafe extern "C" {
    static _trampoline: u8;
}

static KERNEL_PAGE_TABLE: AtomicPtr<PageTable> = AtomicPtr::new(null_mut());

// AArch64 4k-page tables (one table page == 512 64-bit descriptors).
#[repr(C, align(4096))]
#[derive(Copy, Clone)]
pub struct PageTable {
    pub entries: [u64; 512],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    ZeroSize,
    UnalignedAddress,
    UnalignedSize,
    WalkFailed,
    AlreadyMapped,
    AllocationFailed,
}

pub fn k_vm_init() -> Result<*mut PageTable, MapError> {
    let page_table = k_alloc() as *mut PageTable;

    unsafe { core::ptr::write_bytes(page_table as *mut u8, 0, PAGE_SIZE) };

    // map uart no executable and read/write
    map_pages(
        page_table,
        UART0_BASE,
        UART0_BASE,
        PAGE_SIZE as u64,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;

    // map virtio mmio disk interface read/write
    map_pages(
        page_table,
        VIRTIO0 as u64,
        VIRTIO0 as u64,
        (4 * PAGE_SIZE) as u64,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;

    // map GIC no executable and read/write
    map_pages(
        page_table,
        GICD_BASE,
        GICD_BASE,
        GICD_SIZE,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;
    map_pages(
        page_table,
        GICC_BASE,
        GICC_BASE,
        GICC_SIZE,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;

    // map kernel text executable and read-only.
    let text_end = pg_round_up(unsafe { &etext as *const u8 as u64 } as usize) as u64;
    let text_size = text_end - KERNBASE;
    map_pages(
        page_table,
        KERNBASE,
        KERNBASE,
        text_size,
        AF | UXN | ATTRIDX0 | AP_EL1_RO_EL0_NONE,
    )?;

    // map kernel data and the physical RAM we'll make use of.
    map_pages(
        page_table,
        text_end,
        text_end,
        (RAM_END as u64) - text_end,
        AF | PXN | UXN | ATTRIDX0 | AP_EL1_RW_EL0_NONE,
    )?;

    // map the trampoline for trap entry/exit to
    // the highest virtual address in the kernel.
    map_pages(
        page_table,
        TRAMPOLINE,
        unsafe { (&_trampoline as *const u8) as u64 },
        PAGE_SIZE as u64,
        AF | UXN | ATTRIDX0 | AP_EL1_RO_EL0_NONE,
    )?;

    // Allocate and map a kernel stack for each process.
    proc_map_stacks(page_table)?;

    KERNEL_PAGE_TABLE.store(page_table, Ordering::SeqCst);
    Ok(page_table)
}

pub fn k_vm_init_hart() {
    unsafe { enable_mmu(KERNEL_PAGE_TABLE.load(Ordering::SeqCst) as u64) };
}

#[inline]
pub fn k_stack(p: usize) -> u64 {
    TRAMPOLINE - ((p + 1) as u64) * 2 * (PAGE_SIZE as u64)
}

pub fn proc_page_table(p: &Process) -> Result<u64, MapError> {
    let run = k_alloc();

    if run.is_null() {
        return Err(MapError::AllocationFailed);
    }

    let page_table = run as *mut PageTable;
    unsafe { core::ptr::write_bytes(page_table as *mut u8, 0, PAGE_SIZE) };

    // map the trampoline code (for system call return)
    // at the highest user virtual address.
    // only the supervisor uses it, on the way
    // to/from user space, so not PTE_U.
    map_pages(
        page_table,
        TRAMPOLINE,
        unsafe { (&_trampoline as *const u8) as u64 },
        PAGE_SIZE as u64,
        AF | UXN | ATTRIDX0 | AP_EL1_RO_EL0_RO,
    )?; // fixme clean allocated page in case of error

    // map the trapframe page just below the trampoline page, for trampoline.S.
    map_pages(
        page_table,
        TRAP_FRAME,
        p.trap_frame,
        PAGE_SIZE as u64,
        AF | ATTRIDX0 | AP_EL1_RW_EL0_RW,
    )?;

    Ok(page_table as u64)
}

fn map_pages(
    page_table: *mut PageTable,
    va: u64,
    mut pa: u64,
    size: u64,
    perm: u64,
) -> Result<(), MapError> {
    if size == 0 {
        return Err(MapError::ZeroSize);
    }

    if (va % PAGE_SIZE as u64) != 0 || (pa % PAGE_SIZE as u64) != 0 {
        return Err(MapError::UnalignedAddress);
    }

    if (size % PAGE_SIZE as u64) != 0 {
        return Err(MapError::UnalignedSize);
    }

    klog!(
        logger(),
        "vm: map va=0x{:X}..0x{:X} pa=0x{:X}..0x{:X} size={}B pages={}\n",
        va,
        va + size,
        pa,
        pa + size,
        size,
        size / PAGE_SIZE as u64
    );

    let mut a = va;
    let last = va + size - (PAGE_SIZE as u64);
    loop {
        match walk(page_table, a) {
            Some(pte) => {
                let pte = unsafe { &mut *pte };
                if (*pte & DESC_VALID) != 0 {
                    klog!(logger(), "Page already mapped\n",);
                    return Err(MapError::AlreadyMapped);
                }

                *pte = page_address_to_page_entry(pa) | DESC_VALID | DESC_PAGE | perm;
                if a == last {
                    break;
                }

                a += PAGE_SIZE as u64;
                pa += PAGE_SIZE as u64;
            }
            None => return Err(MapError::WalkFailed),
        }
    }

    Ok(())
}

fn proc_map_stacks(page_table: *mut PageTable) -> Result<(), MapError> {
    for i in 0..NUMBER_OF_PROCESS {
        let pa = k_alloc();

        if pa.is_null() {
            panic!("kalloc");
        }

        let va = k_stack(i);
        map_pages(
            page_table,
            va,
            pa as u64,
            PAGE_SIZE as u64,
            AF | PXN | UXN | ATTRIDX0 | AP_EL1_RW_EL0_NONE,
        )?;
    }

    Ok(())
}

#[inline]
fn level_index(level: usize, va: u64) -> usize {
    const SHIFTS: [usize; 4] = [39, 30, 21, 12];
    ((va >> SHIFTS[level]) & 0x1FF) as usize
}

#[inline]
fn page_address_to_page_entry(pa: u64) -> Pte {
    pa & 0x0000_FFFF_FFFF_F000
}

#[inline]
fn pte_to_page_address(pa: u64) -> Pte {
    pa & 0x0000_FFFF_FFFF_F000
}

fn walk(mut page_table: *mut PageTable, va: u64) -> Option<*mut Pte> {
    if va >= MAX_VA {
        panic!("walk");
    }

    for level in 0..3 {
        let idx = level_index(level, va);
        let pte = unsafe { &mut (*page_table).entries[idx] };

        if (*pte & DESC_VALID) != 0 {
            if (*pte & DESC_TABLE) == 0 {
                return None;
            }
            page_table = pte_to_page_address(*pte) as *mut PageTable;
        } else {
            let next = k_alloc();
            unsafe { core::ptr::write_bytes(next as *mut u8, 0, PAGE_SIZE) };
            *pte = page_address_to_page_entry(next as u64) | DESC_VALID | DESC_TABLE;
            page_table = next as *mut PageTable;
        }
    }

    let idx = level_index(3, va);
    let pte = unsafe { &mut (*page_table).entries[idx] };
    Some(pte as *mut Pte)
}
