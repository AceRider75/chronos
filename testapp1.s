bits 64
section .text
global _start

_start:
    ; Chronos OS Syscall 0x80 signals completion and returns to shell
    int 0x80
    
    ; Should never reach here as syscall_handler in Chronos jumps to resume_shell
    jmp _start
