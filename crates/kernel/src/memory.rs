use core::ptr;

unsafe extern "C" {
    // Linker script symbol, e.g. "_end" or "kernel_end"
    // Adjust link_name to whatever your linker script defines.
    #[link_name = "kernel_end"]
    static kernel_end: u8;
}

pub const PAGE_SIZE: usize = 4096;
const RAM_START: usize = 0x4000_0000;
const RAM_SIZE: usize = 256 * 1024 * 1024; // should match qemu memory: -m 256M
pub const RAM_END: usize = RAM_START + RAM_SIZE;

#[repr(C)]
pub struct Run {
    next: *mut Run,
}

#[repr(C)]
struct KMem {
    freelist: *mut Run,
}

static mut K_MEM: KMem = KMem {
    freelist: ptr::null_mut(),
};

#[inline]
pub const fn pg_round_up(sz: usize) -> usize {
    (sz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

pub fn kernel_end_addr() -> usize {
    &raw const kernel_end as usize
}

pub fn k_me_init() {
    free_range(kernel_end_addr(), RAM_END);
}

fn free_range(pa_start: usize, pa_end: usize) {
    let mut pa = pg_round_up(pa_start);

    while pa + PAGE_SIZE <= pa_end {
        k_free(pa as *mut u8);
        pa += PAGE_SIZE;
    }
}

fn k_free(pa: *mut u8) {
    let pa_addr = pa as usize;
    if pa_addr % PAGE_SIZE != 0 || pa_addr < kernel_end_addr() || pa_addr >= RAM_END {
        panic!("kfree");
    }

    // Fill with junk to catch dangling refs.
    unsafe {
        core::ptr::write_bytes(pa, 1, PAGE_SIZE);
    }

    unsafe {
        let r: &mut Run = &mut *(pa as *mut Run);
        r.next = K_MEM.freelist;
        K_MEM.freelist = r;
    }
}

pub fn k_alloc() -> *mut Run {
    unsafe {
        let r: *mut Run = K_MEM.freelist;
        if !r.is_null() {
            K_MEM.freelist = (*r).next;
            core::ptr::write_bytes(r as *mut u8, 5, PAGE_SIZE);
        }
        r
    }
}
