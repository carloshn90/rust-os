use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use hal::klog;

use crate::{
    memory::{PAGE_SIZE, RAM_END, k_alloc, pg_round_up},
    uart_logger::{UART0_BASE, logger},
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

unsafe extern "C" {
    static etext: u8;
}
unsafe extern "C" {
    fn enable_mmu(ttbr0: u64);
}

static KERNEL_PAGE_TABLE: AtomicPtr<PageTable> = AtomicPtr::new(null_mut());

// AArch64 4k-page tables (one table page == 512 64-bit descriptors).
#[repr(C, align(4096))]
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

pub fn k_vm_init() -> Result<*mut PageTable, MapError> {
    let page_table = k_alloc() as *mut PageTable;

    unsafe { core::ptr::write_bytes(page_table as *mut u8, 0, PAGE_SIZE) };

    // map uart no executable and read/write
    map_pages(
        page_table,
        UART0_BASE,
        UART0_BASE,
        PAGE_SIZE as u64,
        AF | PXN | UXN | AP_EL1_RW_EL0_NONE,
    )?;

    // map kernel text executable and read-only.
    let text_end = pg_round_up(unsafe { &etext as *const u8 as u64 } as usize) as u64;
    let text_size = text_end - KERNBASE;
    map_pages(
        page_table,
        KERNBASE,
        KERNBASE,
        text_size,
        AF | UXN | AP_EL1_RO_EL0_NONE,
    )?;

    // map kernel data and the physical RAM we'll make use of.
    map_pages(
        page_table,
        text_end,
        text_end,
        (RAM_END as u64) - text_end,
        AF | PXN | UXN | AP_EL1_RW_EL0_NONE,
    )?;

    KERNEL_PAGE_TABLE.store(page_table, Ordering::SeqCst);
    Ok(page_table)
}

pub fn k_vm_init_hart() {
    unsafe { enable_mmu(KERNEL_PAGE_TABLE.load(Ordering::SeqCst) as u64) };
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

                *pte = page_address_to_page_entry(pa) | DESC_VALID | DESC_PAGE | ATTRIDX1 | perm;
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

pub fn walk(mut page_table: *mut PageTable, va: u64) -> Option<*mut Pte> {
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
