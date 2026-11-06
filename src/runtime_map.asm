
; ---- maps ----
; A map object (after the 16-byte header):
;   +0 entries used (including removed ones)   +8 entry capacity   +16 entries
;   +24 flags (1 = text keys, 2 = values are heap objects)   +32 index   +40 index size (a power of two)
;   +48 live entries
; An entry is 32 bytes: [state: 1 live, 0 removed][hash][key][value]. The index holds entry number + 1
; (0 = empty); removed entries stay in the index chain, so removal needs no tombstone handling.

; rax = new empty map; rcx = flags
rt_map_new proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov ecx, 56
    lea rdx, rt_drop_map
    call rt_alloc
    mov qword ptr [rbp - 16], rax
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax + 24], rcx
    mov qword ptr [rax + 8], 4
    mov qword ptr [rax + 40], 8
    mov ecx, 128
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rcx + 16], rax
    mov ecx, 64
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rcx + 32], rax
    mov rax, rcx
    mov rsp, rbp
    pop rbp
    ret
rt_map_new endp

; rax = hash of key rdx (borrowed) for map rcx
rt_map_hash proc
    test qword ptr [rcx + 24], 1
    jnz rt_mh_text
    mov rax, rdx
    mov rcx, 09E3779B97F4A7C15h
    imul rax, rcx
    mov rcx, rax
    shr rcx, 32
    xor rax, rcx
    ret
rt_mh_text:
    mov rax, 0cbf29ce484222325h
    test rdx, rdx
    jz rt_mh_ret
    mov rcx, qword ptr [rdx]
    lea r8, [rdx + 8]
    mov r9, 0100000001b3h
rt_mh_loop:
    test rcx, rcx
    jz rt_mh_ret
    movzx edx, byte ptr [r8]
    xor rax, rdx
    imul rax, r9
    inc r8
    dec rcx
    jmp rt_mh_loop
rt_mh_ret:
    ret
rt_map_hash endp

; rax = the live entry of map rcx whose key is rdx (borrowed), or 0
rt_map_find proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    call rt_map_hash
    mov qword ptr [rbp - 24], rax
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rcx + 40]
    dec rdx
    and rax, rdx
    mov qword ptr [rbp - 32], rax
rt_mf_probe:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 32]
    mov rdx, qword ptr [rcx + 32]
    mov rax, qword ptr [rdx + rax * 8]
    test rax, rax
    jz rt_mf_none
    dec rax
    shl rax, 5
    add rax, qword ptr [rcx + 16]
    mov qword ptr [rbp - 40], rax
    cmp qword ptr [rax], 0
    je rt_mf_next
    mov rdx, qword ptr [rbp - 24]
    cmp rdx, qword ptr [rax + 8]
    jne rt_mf_next
    test qword ptr [rcx + 24], 1
    jnz rt_mf_text
    mov rdx, qword ptr [rbp - 16]
    cmp rdx, qword ptr [rax + 16]
    je rt_mf_found
    jmp rt_mf_next
rt_mf_text:
    mov rcx, qword ptr [rbp - 16]
    mov rdx, qword ptr [rax + 16]
    call rt_text_eq
    test rax, rax
    jnz rt_mf_found
rt_mf_next:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 32]
    inc rax
    mov rdx, qword ptr [rcx + 40]
    dec rdx
    and rax, rdx
    mov qword ptr [rbp - 32], rax
    jmp rt_mf_probe
rt_mf_found:
    mov rax, qword ptr [rbp - 40]
    jmp rt_mf_done
rt_mf_none:
    xor eax, eax
rt_mf_done:
    mov rsp, rbp
    pop rbp
    ret
rt_map_find endp

; rax = the value stored under key rdx in map rcx (borrowed); panics when there is none
rt_map_get proc
    sub rsp, 40
    call rt_map_find
    test rax, rax
    jz rt_mg_miss
    mov rax, qword ptr [rax + 24]
    add rsp, 40
    ret
rt_mg_miss:
    lea rcx, rt_msg_nokey
    call rt_panic
rt_map_get endp

; rax = 1 when map rcx has key rdx
rt_map_has proc
    sub rsp, 40
    call rt_map_find
    add rsp, 40
    test rax, rax
    setnz al
    movzx eax, al
    ret
rt_map_has endp

; rebuilds map rcx: drops removed entries and doubles the capacity when more than half is live
rt_map_grow proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov rax, qword ptr [rcx + 8]
    mov rdx, qword ptr [rcx + 48]
    add rdx, rdx
    cmp rdx, rax
    jbe rt_mgr_size
    add rax, rax
rt_mgr_size:
    mov qword ptr [rbp - 16], rax
    mov rcx, rax
    shl rcx, 5
    xor edx, edx
    call rt_alloc
    mov qword ptr [rbp - 24], rax
    mov rcx, qword ptr [rbp - 16]
    shl rcx, 4
    xor edx, edx
    call rt_alloc
    mov qword ptr [rbp - 32], rax
    mov qword ptr [rbp - 40], 0
    mov qword ptr [rbp - 48], 0
rt_mgr_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 40]
    cmp rax, qword ptr [rcx]
    jae rt_mgr_done
    shl rax, 5
    add rax, qword ptr [rcx + 16]
    inc qword ptr [rbp - 40]
    cmp qword ptr [rax], 0
    je rt_mgr_loop
    mov rdx, qword ptr [rbp - 48]
    mov r8, rdx
    shl r8, 5
    add r8, qword ptr [rbp - 24]
    mov r9, qword ptr [rax]
    mov qword ptr [r8], r9
    mov r9, qword ptr [rax + 8]
    mov qword ptr [r8 + 8], r9
    mov r10, qword ptr [rax + 16]
    mov qword ptr [r8 + 16], r10
    mov r10, qword ptr [rax + 24]
    mov qword ptr [r8 + 24], r10
    mov r10, qword ptr [rbp - 16]
    add r10, r10
    dec r10
    and r9, r10
    mov r11, qword ptr [rbp - 32]
rt_mgr_slot:
    cmp qword ptr [r11 + r9 * 8], 0
    je rt_mgr_put
    inc r9
    and r9, r10
    jmp rt_mgr_slot
rt_mgr_put:
    inc rdx
    mov qword ptr [r11 + r9 * 8], rdx
    mov qword ptr [rbp - 48], rdx
    jmp rt_mgr_loop
rt_mgr_done:
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 32]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 24]
    mov qword ptr [rcx + 16], rax
    mov rax, qword ptr [rbp - 32]
    mov qword ptr [rcx + 32], rax
    mov rax, qword ptr [rbp - 16]
    mov qword ptr [rcx + 8], rax
    add rax, rax
    mov qword ptr [rcx + 40], rax
    mov rax, qword ptr [rbp - 48]
    mov qword ptr [rcx], rax
    mov rsp, rbp
    pop rbp
    ret
rt_map_grow endp

; map rcx [key rdx (borrowed)] = r8 (an owned value); a new text key is retained
rt_map_set proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov qword ptr [rbp - 24], r8
    call rt_map_find
    test rax, rax
    jz rt_ms_new
    mov qword ptr [rbp - 32], rax
    mov rcx, qword ptr [rbp - 8]
    test qword ptr [rcx + 24], 2
    jz rt_ms_store
    mov rcx, qword ptr [rax + 24]
    call rt_release
rt_ms_store:
    mov rax, qword ptr [rbp - 32]
    mov rcx, qword ptr [rbp - 24]
    mov qword ptr [rax + 24], rcx
    jmp rt_ms_done
rt_ms_new:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rcx]
    cmp rax, qword ptr [rcx + 8]
    jb rt_ms_append
    call rt_map_grow
rt_ms_append:
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rbp - 16]
    call rt_map_hash
    mov qword ptr [rbp - 40], rax
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rcx]
    mov rax, rdx
    shl rax, 5
    add rax, qword ptr [rcx + 16]
    mov qword ptr [rax], 1
    mov r8, qword ptr [rbp - 40]
    mov qword ptr [rax + 8], r8
    mov r8, qword ptr [rbp - 16]
    mov qword ptr [rax + 16], r8
    mov r8, qword ptr [rbp - 24]
    mov qword ptr [rax + 24], r8
    inc qword ptr [rcx]
    inc qword ptr [rcx + 48]
    mov r9, qword ptr [rcx + 40]
    dec r9
    mov r10, qword ptr [rbp - 40]
    and r10, r9
    mov r11, qword ptr [rcx + 32]
rt_ms_slot:
    cmp qword ptr [r11 + r10 * 8], 0
    je rt_ms_put
    inc r10
    and r10, r9
    jmp rt_ms_slot
rt_ms_put:
    inc rdx
    mov qword ptr [r11 + r10 * 8], rdx
    test qword ptr [rcx + 24], 1
    jz rt_ms_done
    mov rcx, qword ptr [rbp - 16]
    call rt_retain
rt_ms_done:
    mov rsp, rbp
    pop rbp
    ret
rt_map_set endp

; removes key rdx from map rcx (nothing happens when it is not there)
rt_map_remove proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    call rt_map_find
    test rax, rax
    jz rt_mr_done
    mov qword ptr [rbp - 16], rax
    mov qword ptr [rax], 0
    mov rcx, qword ptr [rbp - 8]
    dec qword ptr [rcx + 48]
    test qword ptr [rcx + 24], 1
    jz rt_mr_val
    mov rcx, qword ptr [rax + 16]
    call rt_release
rt_mr_val:
    mov rcx, qword ptr [rbp - 8]
    test qword ptr [rcx + 24], 2
    jz rt_mr_done
    mov rax, qword ptr [rbp - 16]
    mov rcx, qword ptr [rax + 24]
    call rt_release
rt_mr_done:
    mov rsp, rbp
    pop rbp
    ret
rt_map_remove endp

; releases the keys and values of the live entries of map rcx
rt_map_release_all proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], 0
rt_mra_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 16]
    cmp rax, qword ptr [rcx]
    jae rt_mra_done
    inc qword ptr [rbp - 16]
    shl rax, 5
    add rax, qword ptr [rcx + 16]
    cmp qword ptr [rax], 0
    je rt_mra_loop
    mov qword ptr [rbp - 24], rax
    test qword ptr [rcx + 24], 1
    jz rt_mra_val
    mov rcx, qword ptr [rax + 16]
    call rt_release
rt_mra_val:
    mov rcx, qword ptr [rbp - 8]
    test qword ptr [rcx + 24], 2
    jz rt_mra_loop
    mov rax, qword ptr [rbp - 24]
    mov rcx, qword ptr [rax + 24]
    call rt_release
    jmp rt_mra_loop
rt_mra_done:
    mov rsp, rbp
    pop rbp
    ret
rt_map_release_all endp

; removes every entry of map rcx
rt_map_clear proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    call rt_map_release_all
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rcx], 0
    mov qword ptr [rcx + 48], 0
    mov r8, qword ptr [rcx + 40]
    mov rdx, qword ptr [rcx + 32]
rt_mcl_loop:
    test r8, r8
    jz rt_mcl_done
    dec r8
    mov qword ptr [rdx + r8 * 8], 0
    jmp rt_mcl_loop
rt_mcl_done:
    mov rsp, rbp
    pop rbp
    ret
rt_map_clear endp

rt_drop_map proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    call rt_map_release_all
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 32]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_map endp
