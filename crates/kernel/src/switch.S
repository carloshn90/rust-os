.section .text
.global switch
.type switch, %function
.align 4

// switch(old: *mut Context, new: *const Context)
// x0 = old
// x1 = new
switch:
    // Save current callee-saved registers into *old
    str x19, [x0, #(19 * 8)]
    str x20, [x0, #(20 * 8)]
    str x21, [x0, #(21 * 8)]
    str x22, [x0, #(22 * 8)]
    str x23, [x0, #(23 * 8)]
    str x24, [x0, #(24 * 8)]
    str x25, [x0, #(25 * 8)]
    str x26, [x0, #(26 * 8)]
    str x27, [x0, #(27 * 8)]
    str x28, [x0, #(28 * 8)]
    str x29, [x0, #(29 * 8)]
    str x30, [x0, #(30 * 8)]

    mov x2, sp
    str x2, [x0, #(31 * 8)]     // old->sp = sp

    // Restore callee-saved registers from *new
    ldr x19, [x1, #(19 * 8)]
    ldr x20, [x1, #(20 * 8)]
    ldr x21, [x1, #(21 * 8)]
    ldr x22, [x1, #(22 * 8)]
    ldr x23, [x1, #(23 * 8)]
    ldr x24, [x1, #(24 * 8)]
    ldr x25, [x1, #(25 * 8)]
    ldr x26, [x1, #(26 * 8)]
    ldr x27, [x1, #(27 * 8)]
    ldr x28, [x1, #(28 * 8)]
    ldr x29, [x1, #(29 * 8)]
    ldr x30, [x1, #(30 * 8)]

    ldr x2, [x1, #(31 * 8)]
    mov sp, x2                  // sp = new->sp

    ret
