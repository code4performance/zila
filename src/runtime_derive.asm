
; ---- standard interfaces: text order and hash ----

; eax = -1, 0 or 1: how text rcx orders against text rdx, byte by byte (a shorter text first on a
; tie); a nil text is empty
rt_dv_text_cmp proc
    push rsi
    xor r8d, r8d
    test rcx, rcx
    jz rt_dvc_a0
    mov r8, qword ptr [rcx]
    add rcx, 8
rt_dvc_a0:
    xor r9d, r9d
    test rdx, rdx
    jz rt_dvc_b0
    mov r9, qword ptr [rdx]
    add rdx, 8
rt_dvc_b0:
    xor r11d, r11d
rt_dvc_loop:
    cmp r11, r8
    jae rt_dvc_tie
    cmp r11, r9
    jae rt_dvc_tie
    movzx eax, byte ptr [rcx + r11]
    movzx esi, byte ptr [rdx + r11]
    cmp eax, esi
    jb rt_dvc_lt
    ja rt_dvc_gt
    inc r11
    jmp rt_dvc_loop
rt_dvc_tie:
    cmp r8, r9
    jb rt_dvc_lt
    ja rt_dvc_gt
    xor eax, eax
    jmp rt_dvc_done
rt_dvc_lt:
    mov eax, -1
    jmp rt_dvc_done
rt_dvc_gt:
    mov eax, 1
rt_dvc_done:
    pop rsi
    ret
rt_dv_text_cmp endp

; rax = the hash of text rcx (FNV-1a, the hash maps use for text keys); nil hashes as an empty text
rt_dv_text_hash proc
    mov rax, 0cbf29ce484222325h
    test rcx, rcx
    jz rt_dvh_ret
    mov rdx, qword ptr [rcx]
    lea r8, [rcx + 8]
    mov r9, 0100000001b3h
rt_dvh_loop:
    test rdx, rdx
    jz rt_dvh_ret
    movzx ecx, byte ptr [r8]
    xor rax, rcx
    imul rax, r9
    inc r8
    dec rdx
    jmp rt_dvh_loop
rt_dvh_ret:
    ret
rt_dv_text_hash endp
