use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use hal::kdebug;

use crate::{
    memory::{PAGE_SIZE, RAM_END, k_alloc, pg_round_up},
    process::{NUMBER_OF_PROCESS, Process},
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
pub const AF: u64 = 1 << 10;
pub const ATTRIDX0: u64 = 0 << 2;
const ATTRIDX1: u64 = 1 << 2;

const AP_EL1_RW_EL0_NONE: u64 = 0b00 << 6;
pub const AP_EL1_RW_EL0_RW: u64 = 0b01 << 6;
#[allow(dead_code)]
const AP_EL1_RW_EL0_RX: u64 = 0b10 << 6;
pub const AP_EL1_RX_EL0_RX: u64 = 0b11 << 6; // Value: 0xC0 (or 192)
const AP_EL1_RO_EL0_NONE: u64 = 0b10 << 6;
const AP_EL1_RO_EL0_RO: u64 = 0b11 << 6;

pub const PXN: u64 = 1 << 53;
pub const UXN: u64 = 1 << 54;

const KERNEL_PHYS_BASE: u64 = 0x4008_0000;
const TRAMPOLINE: u64 = MAX_VA - PAGE_SIZE as u64;
pub const TRAP_FRAME: u64 = TRAMPOLINE - PAGE_SIZE as u64;
pub const KSTACK_PAGES: u64 = 2;
pub const KSTACK_GUARD_PAGES: u64 = 1;
pub const KSTACK_SIZE: u64 = KSTACK_PAGES * PAGE_SIZE as u64;
const GICD_BASE: u64 = 0x0800_0000;
const GICC_BASE: u64 = 0x0801_0000;
const GICD_SIZE: u64 = 0x10000;
const GICC_SIZE: u64 = 0x10000;
const VIRTIO0_BASE: usize = 0x0a000000;

unsafe extern "C" {
    static etext: u8;
}
unsafe extern "C" {
    fn enable_mmu(ttbr0: u64);
}

unsafe extern "C" {
    static _trampoline: u8;
}

pub static KERNEL_PAGE_TABLE: AtomicPtr<PageTable> = AtomicPtr::new(null_mut());

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
        VIRTIO0_BASE as u64,
        VIRTIO0_BASE as u64,
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
    let text_size = text_end - KERNEL_PHYS_BASE;
    map_pages(
        page_table,
        KERNEL_PHYS_BASE,
        KERNEL_PHYS_BASE,
        text_size,
        AF | UXN | ATTRIDX0 | AP_EL1_RO_EL0_NONE,
    )?;

    // map kernel data and the physical RAM we'll make use of.
    map_pages(
        page_table,
        text_end,
        text_end,
        (RAM_END as u64) - text_end,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
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
    let kpt = KERNEL_PAGE_TABLE.load(Ordering::SeqCst) as u64;
    unsafe { enable_mmu(kpt) };
}

#[inline]
pub fn k_stack(p: usize) -> u64 {
    let stride_pages = KSTACK_PAGES + KSTACK_GUARD_PAGES;
    TRAMPOLINE - ((p + 1) as u64) * stride_pages * (PAGE_SIZE as u64)
}

pub fn proc_page_table(p: &Process) -> Result<u64, MapError> {
    let run = k_alloc();

    if run.is_null() {
        return Err(MapError::AllocationFailed);
    }

    let page_table = run as *mut PageTable;
    unsafe { core::ptr::write_bytes(page_table as *mut u8, 0, PAGE_SIZE) };

    let text_end = pg_round_up(unsafe { &etext as *const u8 as u64 } as usize) as u64;
    let text_size = text_end - KERNEL_PHYS_BASE;
    map_pages(
        page_table,
        KERNEL_PHYS_BASE,
        KERNEL_PHYS_BASE,
        text_size,
        AF | UXN | ATTRIDX0 | AP_EL1_RO_EL0_NONE,
    )?;

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
        p.trap_frame as u64,
        PAGE_SIZE as u64,
        AF | ATTRIDX0 | AP_EL1_RW_EL0_RW,
    )?;

    // EPD1=1 means TCR_EL1 has no TTBR1 kernel address space.  When
    // user_trap_return switches TTBR0 to this per-process table the kernel
    // must still reach its own stack, statics and MMIO inside EL1 exception
    // handlers.  Replicate those mappings here with EL0-no-access so user
    // code cannot touch them.

    // Kernel data / BSS / free-RAM (contains globals such as LOGGING_INITIALISED).
    map_pages(
        page_table,
        text_end,
        text_end,
        (RAM_END as u64) - text_end,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;

    // UART MMIO – needed by every l.log() call inside syscall handlers.
    map_pages(
        page_table,
        UART0_BASE,
        UART0_BASE,
        PAGE_SIZE as u64,
        AF | PXN | UXN | ATTRIDX1 | AP_EL1_RW_EL0_NONE,
    )?;

    // GIC distributor and CPU interface – needed by interrupt handlers.
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

    // Kernel stacks: scheduler context-switch may pivot SP to another process
    // stack before switching TTBR0, so every process page table must include
    // all kernel stack mappings (EL0 no-access).
    let kpt = KERNEL_PAGE_TABLE.load(Ordering::SeqCst) as *mut PageTable;
    for i in 0..NUMBER_OF_PROCESS {
        let stack_va = k_stack(i);
        for page in 0..KSTACK_PAGES {
            let va = stack_va + page * PAGE_SIZE as u64;
            let stack_phys = kvm_translate(kpt, va)?;
            map_pages(
                page_table,
                va,
                stack_phys as u64,
                PAGE_SIZE as u64,
                AF | PXN | UXN | ATTRIDX0 | AP_EL1_RW_EL0_NONE,
            )?;
        }
    }

    Ok(page_table as u64)
}

pub fn u_vm_alloc(
    page_table: *mut PageTable,
    old_size: usize,
    new_size: usize,
    perm: u64,
) -> Result<u64, MapError> {
    if new_size < old_size {
        return Ok(old_size as u64);
    }

    let old_size = pg_round_up(old_size);

    for a in (old_size..new_size).step_by(PAGE_SIZE) {
        let mem = k_alloc();
        if mem.is_null() {
            return Err(MapError::AllocationFailed);
        }
        unsafe {
            core::ptr::write_bytes(mem as *mut u8, 0, PAGE_SIZE);
        }

        map_pages(page_table, a as u64, mem as u64, PAGE_SIZE as u64, perm)?;
    }

    Ok(page_table as u64)
}

pub fn clone_user_space(
    parent_page_table: *mut PageTable,
    child_page_table: *mut PageTable,
) -> Result<(), MapError> {
    // Current user binaries are linked in the low user image window and use
    // a fixed one-page stack at 0x8000_0000. Copy only EL0-accessible leaves
    // from those ranges so fork stays fast and bounded.
    const USER_IMAGE_START: u64 = 0x0010_0000;
    const USER_IMAGE_END: u64 = 0x0012_0000;
    const USER_STACK_START: u64 = 0x8000_0000;
    const USER_STACK_END: u64 = USER_STACK_START + PAGE_SIZE as u64;

    clone_user_pages_in_range(
        parent_page_table,
        child_page_table,
        USER_IMAGE_START,
        USER_IMAGE_END,
    )?;
    clone_user_pages_in_range(
        parent_page_table,
        child_page_table,
        USER_STACK_START,
        USER_STACK_END,
    )?;
    Ok(())
}

pub fn kvm_translate(page_table: *mut PageTable, va: u64) -> Result<*mut u8, MapError> {
    // This assumes your existing page table walker can find the Level 3 descriptor.
    // Replace `walk_to_level_3` with your actual page table walking function name.
    let pte_ptr = walk(page_table, va).ok_or(MapError::WalkFailed)?;
    unsafe {
        let pte = *pte_ptr;
        // Ensure the page entry is actually valid/present
        if (pte & 0x1) != 0 {
            // Extract the physical address (Mask out lower attribute bits)
            // In AArch64 Stage 1, OA[47:12] holds the output address
            let phys_addr = pte & 0x0000_FFFF_FFFF_F000;
            // Add the in-page offset from the original virtual address
            let page_offset = va & (PAGE_SIZE as u64 - 1);
            return Ok((phys_addr + page_offset) as *mut u8);
        }
        Err(MapError::WalkFailed)
    }
}

pub fn map_pages(
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

    kdebug!(
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
                    kdebug!(logger(), "Page already mapped\n",);
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
        let va = k_stack(i);
        for page in 0..KSTACK_PAGES {
            let pa = k_alloc();
            if pa.is_null() {
                panic!("kalloc");
            }

            map_pages(
                page_table,
                va + page * PAGE_SIZE as u64,
                pa as u64,
                PAGE_SIZE as u64,
                AF | PXN | UXN | ATTRIDX0 | AP_EL1_RW_EL0_NONE,
            )?;
        }
    }

    Ok(())
}

fn clone_user_pages_in_range(
    parent_page_table: *mut PageTable,
    child_page_table: *mut PageTable,
    start: u64,
    end: u64,
) -> Result<(), MapError> {
    const USER_AP_MASK: u64 = 0b11 << 6;
    const LEAF_ATTR_MASK: u64 = !0x0000_FFFF_FFFF_F000;

    let mut va = start;
    while va < end {
        let Some(parent_pte_ptr) = walk_existing(parent_page_table, va) else {
            va += PAGE_SIZE as u64;
            continue;
        };

        let parent_pte = unsafe { *parent_pte_ptr };
        if (parent_pte & DESC_VALID) == 0 || (parent_pte & DESC_TABLE) != DESC_PAGE {
            va += PAGE_SIZE as u64;
            continue;
        }

        let access = parent_pte & USER_AP_MASK;
        if access != AP_EL1_RW_EL0_RW && access != AP_EL1_RX_EL0_RX {
            va += PAGE_SIZE as u64;
            continue;
        }

        if va == TRAMPOLINE || va == TRAP_FRAME {
            va += PAGE_SIZE as u64;
            continue;
        }

        let perm = parent_pte & LEAF_ATTR_MASK & !(DESC_VALID | DESC_PAGE);
        let parent_pa = pte_to_page_address(parent_pte);
        let child_mem = k_alloc();
        if child_mem.is_null() {
            return Err(MapError::AllocationFailed);
        }
        let child_pa = child_mem as *mut u8;

        unsafe {
            core::ptr::copy_nonoverlapping(parent_pa as *const u8, child_pa, PAGE_SIZE);
        }

        map_pages(
            child_page_table,
            va,
            child_pa as u64,
            PAGE_SIZE as u64,
            perm,
        )?;
        va += PAGE_SIZE as u64;
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

#[inline]
fn is_canonical_addr(va: u64) -> bool {
    let is_lower = (va & 0xFFFF_0000_0000_0000) == 0;
    let is_upper = (va & 0xFFFF_0000_0000_0000) == 0xFFFF_0000_0000_0000;

    is_lower || is_upper
}

fn walk(mut page_table: *mut PageTable, va: u64) -> Option<*mut Pte> {
    if !is_canonical_addr(va) {
        panic!("walk: Non-canonical virtual address 0x{:X}", va);
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

fn walk_existing(mut page_table: *mut PageTable, va: u64) -> Option<*mut Pte> {
    if !is_canonical_addr(va) {
        return None;
    }

    for level in 0..3 {
        let idx = level_index(level, va);
        let pte = unsafe { &mut (*page_table).entries[idx] };

        if (*pte & DESC_VALID) == 0 || (*pte & DESC_TABLE) == 0 {
            return None;
        }

        page_table = pte_to_page_address(*pte) as *mut PageTable;
    }

    let idx = level_index(3, va);
    let pte = unsafe { &mut (*page_table).entries[idx] };
    Some(pte as *mut Pte)
}
