.section .text.boot
.global _start

_start:
  // Set stack
  ldr x0, =__stack_top
  mov sp, x0

  // Zero BSS [__bss_start, __bss_end] 
  ldr x1, =__bss_start
  ldr x2, =__bss_end
1:
  // Initialise the entire BSS region with Zeros.
  cmp x1, x2
  b.ge 2f
  str xzr, [x1], #8 // Store zero to adr in x1, then inc x1 by 8 bytes 
  b 1b
2:
  // If we entered at EL2 (typical for QEMU virt), drop to EL1 so the kernel runs
  // in a simpler environment (EL1 + GICv2 + CNTP timer).
  mrs x1, CurrentEL
  lsr x1, x1, #2
  and x1, x1, #3  // EL, bits[3:2]
  cmp x1, #2      // 0b00 -> EL0, 0b01 -> EL1, 0b10 -> EL2, 0b11 -> EL3
  b.ne el1_start

  // Set up an EL1 statck pointer
  ldr x0, =__stack_top
  msr sp_el1, x0

  // Configure EL2 to return to EL1h
  mov x0, #(0b0101) // EL1h
  msr spsr_el2, x0
  adr x0, el1_start
  msr elr_el2, x0

  // Enable FP/ASIMD access at EL1 and ensure EL2 doesn't trap it 
  mrs x0, cptr_el2
  bic x0, x0, #(1 << 10) // TFP = 0 (don't trap FP/ASIMD)
  msr cptr_el2, x0
  isb
  eret

el1_start:
  // Install a minimal exception vector table for the "current" exception level.
  // QEMU `virt` typically enters at EL2, so we must set VBAR_EL2 (not just VBAR_EL1) 
  adr x0, vectors
  msr vbar_el1, x0
  isb

  // Enable FP/ASIMD for Rust/LLVM.
  // Rust/LLVM may use NEON registers for struct copies/mencpy even in early bring-up.
  // If FP is disabled, this traps with EC=0x07 (FP/ASIMD access trap).
  mrs x0, cpacr_el1
  orr x0, x0, #(3 << 20) // FPEN = 0b11
  msr cpacr_el1, x0
  isb

  bl rust_main
3:
  wfe
  b 3b

// --------------------------------------------------------------------------
// Exception vectors (AArch64):
// The CPU expects 16 entries, each 0x80 bytes apart (total 0x800 bytes).
// If we don't pad correctly, the first real IRQ will jump into garbage.
// --------------------------------------------------------------------------
.align 11
vectors:
  // 0x000: Current EL, SP0, Sync
  b exc_sync
  .space 0x80 - 4
  // 0x080: Current EL, SP0, IRQ
  b exc_irq
  .space 0x80 - 4
  // 0x100: Current EL, SP0, FIQ
  b exc_fiq
  .space 0x80 - 4
  // 0x180: Current EL, SP0, SError
  b exc_serr
  .space 0x80 - 4

  // 0x200: Current EL, SPx, Sync
  b exc_sync
  .space 0x80 - 4
  // 0x280: Current EL, SPx, IRQ
  b exc_irq
  .space 0x80 - 4
  // 0x300: Current EL, SPx, FIQ
  b exc_fiq
  .space 0x80 - 4
  // 0x380: Current EL, SPx, SError
  b exc_serr
  .space 0x80 - 4

  // 0x400: Lower EL (AArch64), Sync
  b exc_sync
  .space 0x80 - 4
  // 0x480: Lower EL (AArch64), IRQ
  b exc_irq
  .space 0x80 - 4
  // 0x500: Lower EL (AArch64), FIQ
  b exc_fiq
  .space 0x80 - 4
  // 0x580: Lower EL (AArch64), SError
  b exc_serr
  .space 0x80 - 4

  // 0x600: Lower EL (AArch32), Sync
  b exc_sync
  .space 0x80 - 4
  // 0x680: Lower EL (AArch32), IRQ
  b exc_irq
  .space 0x80 - 4
  // 0x700: Lower EL (AArch32), FIQ
  b exc_fiq
  .space 0x80 - 4
  // 0x780: Lower EL (AArch32), SError
  b exc_serr
  .space 0x80 - 4

exc_sync:
  wfe
  b exc_sync
exc_irq:
  bl save_context
  // bl rust_irq_handler
  b load_context
exc_fiq:
  b exc_sync
exc_serr:
  b exc_sync

.global save_context
save_context:
  // Use x20 as the Context* base so we can save x20 too.
  mrs x20, tpidr_el1

  // Save x0..x29
  stp x0,  x1,  [x20, #0x00]
  stp x2,  x3,  [x20, #0x10]
  stp x4,  x5,  [x20, #0x20]
  stp x6,  x7,  [x20, #0x30]
  stp x8,  x9,  [x20, #0x40]
  stp x10, x11, [x20, #0x50]
  stp x12, x13, [x20, #0x60]
  stp x14, x15, [x20, #0x70]
  stp x16, x17, [x20, #0x80]
  stp x18, x19, [x20, #0x90]
  stp x20, x21, [x20, #0xA0]   // NOTE: saves x20 into its own slot x[20]
  stp x22, x23, [x20, #0xB0]
  stp x24, x25, [x20, #0xC0]
  stp x26, x27, [x20, #0xD0]
  stp x28, x29, [x20, #0xE0]

  // Save x30
  str x30, [x20, #0xF0]

  // Save SP, ELR, SPSR
  mov x0, sp
  str x0, [x20, #0xF8]
  mrs x0, elr_el1
  str x0, [x20, #0x100]
  mrs x0, spsr_el1
  str x0, [x20, #0x108]

  // Call Rust IRQ handler: x0 = current Context*, returns x0 = next Context*
  mov x0, x20
  ret

.global load_context
load_context:
  // Switch to next context
  msr tpidr_el1, x0
  mov x21, x0        // x21 = next Context* base

  // Restore SP/ELR/SPSR
  ldr x1, [x21, #0xF8]
  mov sp, x1
  ldr x1, [x21, #0x100]
  msr elr_el1, x1
  ldr x1, [x21, #0x108]
  msr spsr_el1, x1

  // Restore x0..x29
  ldp x0,  x1,  [x21, #0x00]
  ldp x2,  x3,  [x21, #0x10]
  ldp x4,  x5,  [x21, #0x20]
  ldp x6,  x7,  [x21, #0x30]
  ldp x8,  x9,  [x21, #0x40]
  ldp x10, x11, [x21, #0x50]
  ldp x12, x13, [x21, #0x60]
  ldp x14, x15, [x21, #0x70]
  ldp x16, x17, [x21, #0x80]
  ldp x18, x19, [x21, #0x90]
  ldp x20, x21, [x21, #0xA0]   // careful: clobbers base, but we're done using it after this point
  ldp x22, x23, [x21, #0xB0]
  ldp x24, x25, [x21, #0xC0]
  ldp x26, x27, [x21, #0xD0]
  ldp x28, x29, [x21, #0xE0]

  // Restore x30
  ldr x30, [x21, #0xF0]
  eret


.global start_first
start_first:
  msr tpidr_el1, x0
  mov x19, x0          // x19 = Context* (keep as base; restore x19 last)
  // Restore SP/ELR/SPSR
  ldr x1, [x19, #0xF8]
  mov sp, x1
  ldr x1, [x19, #0x100]
  msr elr_el1, x1
  ldr x1, [x19, #0x108]
  msr spsr_el1, x1
  // Restore GPRs (x0..x29, x30)
  ldp x0,  x1,  [x19, #0x00]
  ldp x2,  x3,  [x19, #0x10]
  ldp x4,  x5,  [x19, #0x20]
  ldp x6,  x7,  [x19, #0x30]
  ldp x8,  x9,  [x19, #0x40]
  ldp x10, x11, [x19, #0x50]
  ldp x12, x13, [x19, #0x60]
  ldp x14, x15, [x19, #0x70]
  ldr x16,       [x19, #0x80]
  ldp x17, x18,  [x19, #0x88]
  ldp x20, x21, [x19, #0xA0]
  ldp x22, x23, [x19, #0xB0]
  ldp x24, x25, [x19, #0xC0]
  ldp x26, x27, [x19, #0xD0]
  ldp x28, x29, [x19, #0xE0]
  ldr x30,       [x19, #0xF0]
  ldr x19, [x19, #0x98]
  eret
