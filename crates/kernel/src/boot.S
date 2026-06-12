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
  b exc_sync_user
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
  mrs x0, esr_el1
  mrs x1, elr_el1
  mrs x2, far_el1
  bl rust_kernel_sync_fault
1:
  wfe
  b 1b
exc_sync_user:
  // `bl save_context` below will overwrite x30 with the kernel return address,
  // destroying the user LR before save_context can store it.  Stash x30 in
  // tpidrro_el0 (writable from EL1, not used by userspace in this OS) so we can
  // patch the frame slot [frame+0xF0] after save_context returns.
  msr tpidrro_el0, x30
  bl save_context       // x0 points to the TrapFrame
  mov x19, x0           // Keep TrapFrame pointer
  mrs x10, tpidrro_el0  // recover user LR
  str x10, [x19, #0xF0] // patch x30 slot in frame with the real user LR

  // Route only SVC64 (EC=0x15) to syscall handling.
  mrs x9, esr_el1
  lsr x10, x9, #26
  cmp x10, #0x15
  b.ne exc_sync_user_fault

  // rust_syscall_handler(syscall_id, arg0, arg1, ctx)
  ldr x0, [x19, #64]    // tf.x8 -> syscall_id
  ldr x1, [x19, #0]     // tf.x0 -> arg0
  ldr x2, [x19, #8]     // tf.x1 -> arg1
  mov x3, x19           // ctx  = saved-context ptr (for writing return value)
  bl rust_syscall_handler

  mov x0, x19
  b load_context

exc_sync_user_fault:
  mrs x2, far_el1
  ldr x1, [x19, #256]   // faulting ELR
  mov x0, x9            // ESR
  bl rust_user_sync_fault
1:
  wfe
  b 1b
exc_irq:
    msr tpidrro_el0, x30  // same LR-stash pattern as exc_sync_user
    bl save_context
    mrs x10, tpidrro_el0  // recover the LR that was active before the IRQ
    str x10, [x0, #0xF0]  // patch x30 slot (x0 = frame ptr returned by save_context)
    bl rust_irq_handler
    b load_context
exc_fiq:
  b exc_sync
exc_serr:
  b exc_sync

.global save_context
save_context:
  sub sp, sp, #0x130

    stp x0,  x1,  [sp, #0x00]
    stp x2,  x3,  [sp, #0x10]
    stp x4,  x5,  [sp, #0x20]
    stp x6,  x7,  [sp, #0x30]
    stp x8,  x9,  [sp, #0x40]
    stp x10, x11, [sp, #0x50]
    stp x12, x13, [sp, #0x60]
    stp x14, x15, [sp, #0x70]
    stp x16, x17, [sp, #0x80]
    stp x18, x19, [sp, #0x90]
    stp x20, x21, [sp, #0xA0]
    stp x22, x23, [sp, #0xB0]
    stp x24, x25, [sp, #0xC0]
    stp x26, x27, [sp, #0xD0]
    stp x28, x29, [sp, #0xE0]
    str x30,      [sp, #0xF0]

    add x0, sp, #0x130
    str x0, [sp, #0xF8]      // interrupted SP before subtract
    mrs x0, elr_el1
    str x0, [sp, #0x100]
    mrs x0, spsr_el1
    str x0, [sp, #0x108]
    mrs x0, sp_el0
    str x0, [sp, #0x110]
    // Save current user page-table root so load_context can return with the
    // right address space even after sleeping/scheduling to another process.
    mrs x0, ttbr0_el1
    str x0, [sp, #0x118]

    mov x0, sp
    ret
    
.global load_context
load_context:
    mov sp, x0

    ldr x10, [sp, #0x100]
    msr elr_el1, x10
    ldr x10, [sp, #0x108]
    msr spsr_el1, x10
    ldr x10, [sp, #0x110]
    msr sp_el0, x10
    ldr x10, [sp, #0x118]
    msr ttbr0_el1, x10
    dsb ishst
    tlbi vmalle1
    dsb ish
    isb

    ldp x0,  x1,  [sp, #0x00]
    ldp x2,  x3,  [sp, #0x10]
    ldp x4,  x5,  [sp, #0x20]
    ldp x6,  x7,  [sp, #0x30]
    ldp x8,  x9,  [sp, #0x40]
    ldp x10, x11, [sp, #0x50]
    ldp x12, x13, [sp, #0x60]
    ldp x14, x15, [sp, #0x70]
    ldp x16, x17, [sp, #0x80]
    ldp x18, x19, [sp, #0x90]
    ldp x20, x21, [sp, #0xA0]
    ldp x22, x23, [sp, #0xB0]
    ldp x24, x25, [sp, #0xC0]
    ldp x26, x27, [sp, #0xD0]
    ldp x28, x29, [sp, #0xE0]
    ldr x30,      [sp, #0xF0]

    ldr x11, [sp, #0xF8]
    add sp, sp, #0x130
    mov sp, x11
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

.global user_trap_return
.type user_trap_return, %function
user_trap_return:
    // x0 points to the TrapFrame struct (Must be a high-memory kernel VA via TTBR1)
    
    // 1. SWITCH PAGE TABLE TO THE USER PROCESS SPACE
    ldr x1, [x0, #272]      // Load tf.ttbr0 (contains process root PA + ASID)
    msr ttbr0_el1, x1       // Update user space page table register

  // We currently run with ASID 0 for all processes. Flush stale stage-1 EL1
  // translations after TTBR0 switch so exec/fork cannot observe old mappings.
  dsb ishst
  tlbi vmalle1
  dsb ish
  isb                     // Context synchronization barrier to apply the new TTBR0

    // 2. Restore system registers for EL0
    ldr x1, [x0, #248]      // Load tf.spsr
    msr spsr_el1, x1        
    
    ldr x2, [x0, #256]      // Load tf.epc (User entry point)
    msr elr_el1, x2         
    
    ldr x3, [x0, #264]      // Load tf.sp (User stack pointer)
    msr sp_el0, x3          
    
    // 3. Restore general purpose registers x1 through x30
    ldp x1, x2, [x0, #8]
    ldp x3, x4, [x0, #24]
    ldp x5, x6, [x0, #40]
    ldp x7, x8, [x0, #56]
    ldp x9, x10, [x0, #72]
    ldp x11, x12, [x0, #88]
    ldp x13, x14, [x0, #104]
    ldp x15, x16, [x0, #120]
    ldp x17, x18, [x0, #136]
    ldp x19, x20, [x0, #152]
    ldp x21, x22, [x0, #168]
    ldp x23, x24, [x0, #184]
    ldp x25, x26, [x0, #200]
    ldp x27, x28, [x0, #216]
    ldp x29, x30, [x0, #232]
    
    // 4. Safely restore x0 last from the high-memory structure
    ldr x0, [x0, #0]
    
    // 5. Synchronize architectural state changes 
    isb
    
    // 6. Drop down into EL0!
    eret
