section .data
msg db "Hello from User Space!", 10
len equ $ - msg

section .text
global _start
_start:
    ; Syscall 1 (PRINT)
    mov rax, 1
    mov rdi, msg
    mov rsi, len
    int 0x80

    ; Chronos OS Syscall 2 (EXIT)
    mov rax, 2
    int 0x80
    
    ; Should never reach here
    jmp _start
