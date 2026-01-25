bits 64
global context_switch
section .text

; void context_switch(uint64_t* old_stack_ptr_addr, uint64_t new_stack_ptr);
; Arguments (System V AMD64 ABI):
; RDI = Address where we should save the OLD stack pointer
; RSI = The NEW stack pointer value we want to load

context_switch:
    ; 1. Save Registers of the OLD task
    ; We push them onto the OLD stack
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; 2. Save the current Stack Pointer (RSP) 
    ; Write RSP into the memory address provided by RDI (e.g. &process.stack_pointer)
    mov [rdi], rsp

    ; 3. SWITCH STACKS!
    ; Load the new stack pointer from RSI
    mov rsp, rsi

    ; 4. Restore Registers of the NEW task
    ; We pop them from the NEW stack (which must be set up previously)
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    ; 5. Return 
    ; This pops RIP from the stack and jumps to it
    ret