use hal::klog;

use crate::{uart_logger::logger, virtio_disk::VIRTIO0};

const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
const VIRTIO_MMIO_VERSION: usize = 0x004;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;

#[inline]
fn mmio_read(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[allow(dead_code)]
pub fn virtio_probe() {
    let magic_addr = VIRTIO0 + VIRTIO_MMIO_MAGIC_VALUE;
    let version_addr = VIRTIO0 + VIRTIO_MMIO_VERSION;
    let device_addr = VIRTIO0 + VIRTIO_MMIO_DEVICE_ID;
    let vendor_addr = VIRTIO0 + VIRTIO_MMIO_VENDOR_ID;

    klog!(logger(), "virtio addr magic   = 0x{:016x}\n", magic_addr);
    klog!(logger(), "virtio addr version = 0x{:016x}\n", version_addr);
    klog!(logger(), "virtio addr device  = 0x{:016x}\n", device_addr);
    klog!(logger(), "virtio addr vendor  = 0x{:016x}\n", vendor_addr);

    let magic = mmio_read(magic_addr);
    let version = mmio_read(version_addr);
    let device_id = mmio_read(device_addr);
    let vendor_id = mmio_read(vendor_addr);

    klog!(
        logger(),
        "virtio-mmio: magic=0x{:08x}, version={}, device_id={}, vendor_id=0x{:08x}\n",
        magic,
        version,
        device_id,
        vendor_id
    );
}
