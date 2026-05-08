
// --------------------------------------------------------------------------
// enable_mmu(ttbr0: u64)
// - sets MAIR_EL1/TCR_EL1/TTBR0_EL1
// - invalidates TLB
// - enables SCTLR_EL1.M (caches left OFF for safety)
// --------------------------------------------------------------------------
.global enable_mmu
enable_mmu:
  // MAIR_EL1:
  // AttrIdx0 = 0xFF (Normal WBWA)
  // AttrIdx1 = 0x04 (Device-nGnRE)
  mov x1, #0xFF
  mov x2, #0x04
  lsl x2, x2, #8
  orr x1, x1, x2
  msr mair_el1, x1

  // TCR_EL1:
  // - T0SZ = 16 (48-bit VA)
  // - TG0  = 4k
  // - SH0  = inner shareable
  // - IRGN0/ORGN0 = WBWA
  // - EPD1 = 1 (disable TTBR1)
  // - IPS = 40-bit
  mov x1, #16                  // T0SZ
  mov x2, #(0b11)              // SH0
  lsl x2, x2, #12
  orr x1, x1, x2
  mov x2, #(0b01)              // ORGN0
  lsl x2, x2, #10
  orr x1, x1, x2
  mov x2, #(0b01)              // IRGN0
  lsl x2, x2, #8
  orr x1, x1, x2
  mov x2, #(1)                 // EPD1
  lsl x2, x2, #23
  orr x1, x1, x2
  mov x2, #(0b010)             // IPS=40-bit
  lsl x2, x2, #32
  orr x1, x1, x2
  msr tcr_el1, x1

  // TTBR0_EL1 (x0 = L0 base)
  msr ttbr0_el1, x0

  // Synchronize and invalidate
  dsb sy
  isb
  tlbi vmalle1
  dsb ish
  isb

  // Enable MMU (M=1). Keep caches OFF initially (C=0, I=0).
  mrs x1, sctlr_el1
  orr x1, x1, #1              // M
  bic x1, x1, #(1 << 2)        // C
  bic x1, x1, #(1 << 12)       // I
  msr sctlr_el1, x1
  isb
  ret
