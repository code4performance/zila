//! Part of the code generator (`codegen_x64`): see the section comment below.

// ---------------------------------------------------------------------------
// Runtime (hand-written assembly, linked into every program)
// ---------------------------------------------------------------------------

pub(super) const RUNTIME_COMMON: &str = r#"
; ---- Zila Lang runtime ----

; +1 reference on the object at rcx (null is fine). Preserves all registers.
rt_retain proc
    test rcx, rcx
    jz rt_retain_done
    inc qword ptr [rcx - 16]
rt_retain_done:
    ret
rt_retain endp

; -1 reference on the object at rcx; frees it (and its contents) at zero.
; Preserves rax. Clobbers rcx, rdx, r8-r11.
rt_release proc
    test rcx, rcx
    jz rt_rel_ret
    dec qword ptr [rcx - 16]
    jnz rt_rel_ret
    push rbp
    mov rbp, rsp
    push rax
    push rcx
    sub rsp, 32
    mov rdx, qword ptr [rcx - 8]
    test rdx, rdx
    jz rt_rel_free
    call rdx
rt_rel_free:
    mov rcx, qword ptr [rbp - 16]
    call rt_free_obj
    mov rax, qword ptr [rbp - 8]
    mov rsp, rbp
    pop rbp
rt_rel_ret:
    ret
rt_release endp

; drop procedures (called by rt_release with the object in rcx)
rt_drop_closure proc
    mov rcx, qword ptr [rcx + 8]
    jmp rt_release
rt_drop_closure endp

rt_drop_error proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 8]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_error endp

; rax = the interned kind named by text rcx, +1 reference for the caller (rcx is not consumed).
; Equal names give the same object, so kinds compare by address.
rt_kind_intern proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rax, qword ptr rt_kinds
rt_ki_loop:
    test rax, rax
    jz rt_ki_new
    mov qword ptr [rbp - 16], rax
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rax + 8]
    call rt_text_eq
    test rax, rax
    jnz rt_ki_found
    mov rax, qword ptr [rbp - 16]
    mov rax, qword ptr [rax]
    jmp rt_ki_loop
rt_ki_found:
    mov rax, qword ptr [rbp - 16]
    mov rax, qword ptr [rax + 8]
    mov rcx, rax
    call rt_retain
    mov rsp, rbp
    pop rbp
    ret
rt_ki_new:
    mov ecx, 16
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax + 8], rcx
    mov rdx, qword ptr rt_kinds
    mov qword ptr [rax], rdx
    mov qword ptr rt_kinds, rax
    call rt_retain
    call rt_retain
    mov rax, qword ptr [rbp - 8]
    mov rsp, rbp
    pop rbp
    ret
rt_kind_intern endp

; rax = the kind `std.Other` (+1 reference): the kind of error("message"). It is the first
; entry of the kind table, a static node, so it costs no allocation.
rt_kind_other proc
    lea rax, rt_kind_other_name
    mov rcx, rax
    call rt_retain
    ret
rt_kind_other endp

; rax = 1 when the error rcx or one of its causes has kind rdx
rt_error_is proc
rt_eis_loop:
    test rcx, rcx
    jz rt_eis_no
    cmp qword ptr [rcx + 8], rdx
    je rt_eis_yes
    mov rcx, qword ptr [rcx + 16]
    jmp rt_eis_loop
rt_eis_yes:
    mov eax, 1
    ret
rt_eis_no:
    xor eax, eax
    ret
rt_error_is endp

; rax = new text: the messages of error rcx and its causes, joined with ": "
rt_error_full proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx + 16]
    test rcx, rcx
    jnz rt_ef_chain
    mov rax, qword ptr [rbp - 8]
    mov rax, qword ptr [rax]
    mov rcx, rax
    call rt_retain
    mov rsp, rbp
    pop rbp
    ret
rt_ef_chain:
    call rt_error_full
    mov qword ptr [rbp - 16], rax
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx]
    lea rdx, rt_msg_sep
    call rt_concat
    mov qword ptr [rbp - 24], rax
    mov rcx, rax
    mov rdx, qword ptr [rbp - 16]
    call rt_concat
    mov qword ptr [rbp - 32], rax
    mov rcx, qword ptr [rbp - 24]
    call rt_release
    mov rcx, qword ptr [rbp - 16]
    call rt_release
    mov rax, qword ptr [rbp - 32]
    mov rsp, rbp
    pop rbp
    ret
rt_error_full endp

rt_drop_result_plain proc
    mov rcx, qword ptr [rcx]
    jmp rt_release
rt_drop_result_plain endp

rt_drop_result_rc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, qword ptr [rcx]
    call rt_release
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 8]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_result_rc endp

; copy r8 bytes from rdx to rcx
rt_memcpy proc
    push rsi
    push rdi
    mov rdi, rcx
    mov rsi, rdx
    mov rcx, r8
    rep movsb
    pop rdi
    pop rsi
    ret
rt_memcpy endp

; write "abort: <location rdx>: <message rcx>" to standard error and exit with code 101
; (rdx = 0 when there is no location). Nothing else runs: no defer, no drop, no flush.
rt_abort proc
    and rsp, -16
    sub rsp, 48
    mov qword ptr [rsp + 32], rcx
    mov qword ptr [rsp + 40], rdx
    lea rcx, rt_msg_abort
    call rt_write_err
    mov rdx, qword ptr [rsp + 40]
    test rdx, rdx
    jz rt_abort_msg
    mov rcx, rdx
    call rt_write_err
    lea rcx, rt_msg_sep
    call rt_write_err
rt_abort_msg:
    mov rcx, qword ptr [rsp + 32]
    call rt_write_err
    lea rcx, rt_msg_nl
    call rt_write_err
    mov ecx, 101
    call rt_exit
rt_abort endp

; rax = new text with room for rcx bytes (zeroed, length set)
rt_text_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    cmp rcx, qword ptr rt_max_len
    ja rt_too_long
    add rcx, 9
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax], rcx
    mov rsp, rbp
    pop rbp
    ret
rt_text_alloc endp

; rax = new text: text rcx + text rdx (operands are not consumed)
rt_concat proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 48
    mov qword ptr [rbp - 24], rcx
    mov qword ptr [rbp - 32], rdx
    mov rcx, qword ptr [rcx]
    add rcx, qword ptr [rdx]
    call rt_text_alloc
    lea rdi, [rax + 8]
    mov rsi, qword ptr [rbp - 24]
    mov rcx, qword ptr [rsi]
    add rsi, 8
    rep movsb
    mov rsi, qword ptr [rbp - 32]
    mov rcx, qword ptr [rsi]
    add rsi, 8
    rep movsb
    add rsp, 48
    pop rdi
    pop rsi
    pop rbp
    ret
rt_concat endp

; rax = 1 if texts rcx and rdx are equal
rt_text_eq proc
    push rsi
    push rdi
    cmp rcx, rdx
    je rt_teq_yes
    test rcx, rcx
    jz rt_teq_no
    test rdx, rdx
    jz rt_teq_no
    mov rax, qword ptr [rcx]
    cmp rax, qword ptr [rdx]
    jne rt_teq_no
    lea rsi, [rcx + 8]
    lea rdi, [rdx + 8]
    mov rcx, rax
    repe cmpsb
    jne rt_teq_no
rt_teq_yes:
    mov eax, 1
    jmp rt_teq_done
rt_teq_no:
    xor eax, eax
rt_teq_done:
    pop rdi
    pop rsi
    ret
rt_text_eq endp

; rax = byte rdx of text rcx (r9 = location); aborts out of range
rt_text_byte_at proc
    cmp rdx, qword ptr [rcx]
    jae rt_tba_oob
    movzx eax, byte ptr [rcx + rdx + 8]
    ret
rt_tba_oob:
    lea rcx, rt_msg_bytepos
    lea r8, rt_msg_outofrange
    jmp rt_abort_val
rt_text_byte_at endp

; abort "<text rcx><number rdx><text r8>" at location r9
rt_abort_val proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], r8
    mov qword ptr [rbp - 24], r9
    mov rcx, rdx
    call rt_text_from_int
    mov rdx, rax
    mov rcx, qword ptr [rbp - 8]
    call rt_concat
    mov rcx, rax
    mov rdx, qword ptr [rbp - 16]
    call rt_concat
    mov rcx, rax
    mov rdx, qword ptr [rbp - 24]
    jmp rt_abort
rt_abort_val endp

; rax = new text: bytes [rdx, r8) of text rcx (r9 = location); aborts when a position is out of
; range, when rdx > r8 or when one is inside a multibyte character
rt_text_slice proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov qword ptr [rbp - 24], r8
    mov qword ptr [rbp - 48], r9
    mov rax, qword ptr [rcx]
    cmp rdx, rax
    ja rt_ts_oob1
    cmp r8, rax
    ja rt_ts_oob2
    cmp rdx, r8
    ja rt_ts_order
    cmp rdx, rax
    jae rt_ts_b1
    movzx r10d, byte ptr [rcx + rdx + 8]
    and r10d, 192
    cmp r10d, 128
    je rt_ts_cut1
rt_ts_b1:
    cmp r8, rax
    jae rt_ts_ok
    movzx r10d, byte ptr [rcx + r8 + 8]
    and r10d, 192
    cmp r10d, 128
    je rt_ts_cut2
rt_ts_ok:
    sub r8, rdx
    mov qword ptr [rbp - 24], r8
    mov rcx, r8
    call rt_text_alloc
    mov qword ptr [rbp - 32], rax
    lea rcx, [rax + 8]
    mov rdx, qword ptr [rbp - 8]
    add rdx, 8
    add rdx, qword ptr [rbp - 16]
    mov r8, qword ptr [rbp - 24]
    call rt_memcpy
    mov rax, qword ptr [rbp - 32]
    mov rsp, rbp
    pop rbp
    ret
rt_ts_oob1:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_outofrange
    jmp rt_ts_fail
rt_ts_oob2:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_outofrange
    mov rdx, qword ptr [rbp - 24]
    jmp rt_ts_fail
rt_ts_order:
    lea rcx, rt_msg_slicestart
    lea r8, rt_msg_afterend
    jmp rt_ts_fail
rt_ts_cut1:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_notboundary
    jmp rt_ts_fail
rt_ts_cut2:
    lea rcx, rt_msg_textpos
    lea r8, rt_msg_notboundary
    mov rdx, qword ptr [rbp - 24]
rt_ts_fail:
    mov r9, qword ptr [rbp - 48]
    jmp rt_abort_val
rt_text_slice endp

; like rt_text_slice without the character-boundary check: for protocol and buffer code that
; cuts bytes, not characters (the result may be invalid UTF-8)
rt_text_slice_raw proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov qword ptr [rbp - 24], r8
    mov qword ptr [rbp - 48], r9
    mov rax, qword ptr [rcx]
    cmp rdx, rax
    ja rt_ts_oob1
    cmp r8, rax
    ja rt_ts_oob2
    cmp rdx, r8
    ja rt_ts_order
    jmp rt_ts_ok
rt_text_slice_raw endp

; rax = new text holding the UTF-8 bytes of code point rcx
rt_text_from_char proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov eax, 1
    cmp ecx, 128
    jb rt_tchr_len
    mov eax, 2
    cmp ecx, 2048
    jb rt_tchr_len
    mov eax, 3
    cmp ecx, 65536
    jb rt_tchr_len
    mov eax, 4
rt_tchr_len:
    mov qword ptr [rbp - 16], rax
    mov rcx, rax
    call rt_text_alloc
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rbp - 16]
    cmp rdx, 1
    jne rt_tchr_2
    mov byte ptr [rax + 8], cl
    jmp rt_tchr_done
rt_tchr_2:
    cmp rdx, 2
    jne rt_tchr_3
    mov r8d, ecx
    shr r8d, 6
    or r8d, 192
    mov byte ptr [rax + 8], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 9], cl
    jmp rt_tchr_done
rt_tchr_3:
    cmp rdx, 3
    jne rt_tchr_4
    mov r8d, ecx
    shr r8d, 12
    or r8d, 224
    mov byte ptr [rax + 8], r8b
    mov r8d, ecx
    shr r8d, 6
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 9], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 10], cl
    jmp rt_tchr_done
rt_tchr_4:
    mov r8d, ecx
    shr r8d, 18
    or r8d, 240
    mov byte ptr [rax + 8], r8b
    mov r8d, ecx
    shr r8d, 12
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 9], r8b
    mov r8d, ecx
    shr r8d, 6
    and r8d, 63
    or r8d, 128
    mov byte ptr [rax + 10], r8b
    and ecx, 63
    or ecx, 128
    mov byte ptr [rax + 11], cl
rt_tchr_done:
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_char endp

; eax = the code point of text rcx at offset rdx (rdx < length), rdx = the offset after it.
; Bytes that are not valid UTF-8 decode as U+FFFD, one per byte. Clobbers rcx, r8-r11.
rt_text_next_char proc
    mov r8, qword ptr [rcx]
    lea r9, [rcx + rdx + 8]
    sub r8, rdx
    movzx eax, byte ptr [r9]
    cmp eax, 128
    jb rt_nc_1
    cmp eax, 194
    jb rt_nc_bad
    cmp eax, 224
    jb rt_nc_2
    cmp eax, 240
    jb rt_nc_3
    cmp eax, 244
    ja rt_nc_bad
    cmp r8, 4
    jb rt_nc_bad
    and eax, 7
    mov r10d, 4
    mov r11d, 65536
    jmp rt_nc_cont
rt_nc_3:
    cmp r8, 3
    jb rt_nc_bad
    and eax, 15
    mov r10d, 3
    mov r11d, 2048
    jmp rt_nc_cont
rt_nc_2:
    cmp r8, 2
    jb rt_nc_bad
    and eax, 31
    mov r10d, 2
    mov r11d, 128
rt_nc_cont:
    mov ecx, 1
rt_nc_loop:
    movzx r8d, byte ptr [r9 + rcx]
    xor r8d, 128
    test r8d, 192
    jnz rt_nc_bad
    shl eax, 6
    or eax, r8d
    inc rcx
    cmp rcx, r10
    jb rt_nc_loop
    cmp eax, r11d
    jb rt_nc_bad
    cmp eax, 1114111
    ja rt_nc_bad
    lea r8d, [rax - 55296]
    cmp r8d, 2047
    jbe rt_nc_bad
    add rdx, r10
    ret
rt_nc_bad:
    mov eax, 65533
rt_nc_1:
    inc rdx
    ret
rt_text_next_char endp

; rax = number of characters of text rcx (an invalid byte counts as one)
rt_text_char_count proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 16
    mov rsi, rcx
    mov qword ptr [rbp - 24], 0
    xor edi, edi
rt_cc_loop:
    mov rdx, rdi
    mov rcx, rsi
    cmp rdx, qword ptr [rsi]
    jae rt_cc_done
    call rt_text_next_char
    mov rdi, rdx
    inc qword ptr [rbp - 24]
    jmp rt_cc_loop
rt_cc_done:
    mov rax, qword ptr [rbp - 24]
    add rsp, 16
    pop rdi
    pop rsi
    pop rbp
    ret
rt_text_char_count endp

; rax = new []byte holding the bytes of text rcx
rt_text_bytes proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    xor ecx, ecx
    call rt_list_new
    mov qword ptr [rbp - 16], rax
    mov qword ptr [rbp - 24], 0
rt_tb_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 24]
    cmp rax, qword ptr [rcx]
    jae rt_tb_done
    movzx edx, byte ptr [rcx + rax + 8]
    mov rcx, qword ptr [rbp - 16]
    call rt_list_push
    inc qword ptr [rbp - 24]
    jmp rt_tb_loop
rt_tb_done:
    mov rax, qword ptr [rbp - 16]
    mov rsp, rbp
    pop rbp
    ret
rt_text_bytes endp

; rax = new text holding the rdx bytes at rcx
rt_text_from_bytes proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rcx, rdx
    call rt_text_alloc
    mov qword ptr [rbp - 24], rax
    lea rcx, [rax + 8]
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    call rt_memcpy
    mov rax, qword ptr [rbp - 24]
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_bytes endp

; rax = decimal text of signed integer rcx
rt_text_from_int proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov rax, rcx
    lea r8, [rbp - 1]
    xor r9d, r9d
    test rax, rax
    jns rt_tfi_pos
    neg rax
    mov r9d, 1
rt_tfi_pos:
    mov r10, 10
rt_tfi_loop:
    xor edx, edx
    div r10
    add dl, 48
    dec r8
    mov byte ptr [r8], dl
    test rax, rax
    jnz rt_tfi_loop
    test r9d, r9d
    jz rt_tfi_copy
    dec r8
    mov byte ptr [r8], 45
rt_tfi_copy:
    lea rcx, [rbp - 1]
    sub rcx, r8
    mov qword ptr [rbp - 40], r8
    mov qword ptr [rbp - 48], rcx
    call rt_text_alloc
    mov rcx, qword ptr [rbp - 48]
    mov r8, qword ptr [rbp - 40]
    lea r9, [rax + 8]
rt_tfi_cp:
    test rcx, rcx
    jz rt_tfi_done
    mov dl, byte ptr [r8]
    mov byte ptr [r9], dl
    inc r8
    inc r9
    dec rcx
    jmp rt_tfi_cp
rt_tfi_done:
    mov rsp, rbp
    pop rbp
    ret
rt_text_from_int endp

; rax = text copied from NUL-terminated bytes at rcx
rt_text_from_cstr proc
    push rbp
    mov rbp, rsp
    push rsi
    push rdi
    sub rsp, 32
    mov rsi, rcx
    xor ecx, ecx
    test rsi, rsi
    jz rt_tfc_alloc
rt_tfc_len:
    cmp byte ptr [rsi + rcx], 0
    je rt_tfc_alloc
    inc rcx
    jmp rt_tfc_len
rt_tfc_alloc:
    mov rdi, rcx
    call rt_text_alloc
    mov rcx, rdi
    lea rdi, [rax + 8]
    rep movsb
    add rsp, 32
    pop rdi
    pop rsi
    pop rbp
    ret
rt_text_from_cstr endp

; rax = new empty list; rcx = 1 when its elements are heap objects
rt_list_new proc
    sub rsp, 40
    mov qword ptr [rsp + 32], rcx
    mov ecx, 32
    lea rdx, rt_drop_list_plain
    cmp qword ptr [rsp + 32], 0
    je rt_ln_alloc
    lea rdx, rt_drop_list_rc
rt_ln_alloc:
    call rt_alloc
    mov rcx, qword ptr [rsp + 32]
    mov qword ptr [rax + 24], rcx
    add rsp, 40
    ret
rt_list_new endp

rt_drop_list_plain proc
    mov rcx, qword ptr [rcx + 16]
    jmp rt_release
rt_drop_list_plain endp

rt_drop_list_rc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], 0
rt_dlr_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 16]
    cmp rax, qword ptr [rcx]
    jae rt_dlr_done
    mov rdx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rdx + rax * 8]
    call rt_release
    inc qword ptr [rbp - 16]
    jmp rt_dlr_loop
rt_dlr_done:
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    call rt_release
    mov rsp, rbp
    pop rbp
    ret
rt_drop_list_rc endp

; append rdx (an owned value) to list rcx
rt_list_push proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rax, qword ptr [rcx]
    cmp rax, qword ptr rt_max_len
    jae rt_too_long
    cmp rax, qword ptr [rcx + 8]
    jb rt_lp_store
    mov rax, qword ptr [rcx + 8]
    add rax, rax
    cmp rax, 4
    jae rt_lp_cap
    mov eax, 4
rt_lp_cap:
    mov qword ptr [rbp - 24], rax
    lea rcx, [rax * 8]
    xor edx, edx
    call rt_alloc
    mov qword ptr [rbp - 32], rax
    mov rcx, qword ptr [rbp - 8]
    mov r8, qword ptr [rcx + 16]
    mov qword ptr [rbp - 40], r8
    mov r9, qword ptr [rcx]
    xor r10d, r10d
rt_lp_cp:
    cmp r10, r9
    jae rt_lp_cpdone
    mov r11, qword ptr [r8 + r10 * 8]
    mov qword ptr [rax + r10 * 8], r11
    inc r10
    jmp rt_lp_cp
rt_lp_cpdone:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 32]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 8], rdx
    mov rcx, qword ptr [rbp - 40]
    call rt_release
rt_lp_store:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rcx]
    mov r8, qword ptr [rcx + 16]
    mov rdx, qword ptr [rbp - 16]
    mov qword ptr [r8 + rax * 8], rdx
    inc qword ptr [rcx]
    mov rsp, rbp
    pop rbp
    ret
rt_list_push endp

; rax = element rdx of list rcx (borrowed)
rt_list_get proc
    cmp rdx, qword ptr [rcx]
    jae rt_index_oob
    mov rax, qword ptr [rcx + 16]
    mov rax, qword ptr [rax + rdx * 8]
    ret
rt_list_get endp

; list rcx [rdx] = r8 (owned); releases the previous element
rt_list_set proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    cmp rdx, qword ptr [rcx]
    jae rt_index_oob
    mov rax, qword ptr [rcx + 16]
    lea rax, [rax + rdx * 8]
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], r8
    cmp qword ptr [rcx + 24], 0
    je rt_ls_store
    mov rcx, qword ptr [rax]
    call rt_release
rt_ls_store:
    mov rax, qword ptr [rbp - 8]
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax], rcx
    mov rsp, rbp
    pop rbp
    ret
rt_list_set endp

; rax = the last element of list rcx, removed WITHOUT releasing it (ownership moves to the caller)
rt_list_take proc
    mov rax, qword ptr [rcx]
    test rax, rax
    jz rt_take_empty
    dec rax
    mov qword ptr [rcx], rax
    mov rdx, qword ptr [rcx + 16]
    mov rax, qword ptr [rdx + rax * 8]
    ret
rt_take_empty:
    lea rcx, rt_msg_take
    jmp rt_abort
rt_list_take endp

; empties list rcx (releasing its elements)
rt_list_clear proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], 0
    cmp qword ptr [rcx + 24], 0
    je rt_lc_done
rt_lc_loop:
    mov rcx, qword ptr [rbp - 8]
    mov rax, qword ptr [rbp - 16]
    cmp rax, qword ptr [rcx]
    jae rt_lc_done
    mov rdx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rdx + rax * 8]
    call rt_release
    inc qword ptr [rbp - 16]
    jmp rt_lc_loop
rt_lc_done:
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rcx], 0
    mov rsp, rbp
    pop rbp
    ret
rt_list_clear endp

; rax = a new list with the elements of list rcx (rdx = function that copies a heap element, or 0 to retain it)
rt_list_clone proc
    push rbp
    mov rbp, rsp
    sub rsp, 96
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rcx, qword ptr [rcx + 24]
    call rt_list_new
    mov qword ptr [rbp - 24], rax
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx]
    test rcx, rcx
    jz rt_lcl_done
    mov qword ptr [rbp - 32], rcx
    shl rcx, 3
    xor edx, edx
    call rt_alloc
    mov rcx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 32]
    mov qword ptr [rcx + 8], rdx
    mov qword ptr [rcx], rdx
    mov qword ptr [rbp - 40], 0
rt_lcl_loop:
    mov rax, qword ptr [rbp - 40]
    cmp rax, qword ptr [rbp - 32]
    jae rt_lcl_done
    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    mov rcx, qword ptr [rcx + rax * 8]
    mov rdx, qword ptr [rbp - 24]
    cmp qword ptr [rdx + 24], 0
    je rt_lcl_store
    mov rax, qword ptr [rbp - 16]
    test rax, rax
    jz rt_lcl_retain
    call rax
    mov rcx, rax
    jmp rt_lcl_store
rt_lcl_retain:
    call rt_retain
rt_lcl_store:
    mov rax, qword ptr [rbp - 24]
    mov rax, qword ptr [rax + 16]
    mov rdx, qword ptr [rbp - 40]
    mov qword ptr [rax + rdx * 8], rcx
    inc qword ptr [rbp - 40]
    jmp rt_lcl_loop
rt_lcl_done:
    mov rax, qword ptr [rbp - 24]
    mov rsp, rbp
    pop rbp
    ret
rt_list_clone endp

; copy on write: rax = the list in the slot at rcx, copied into the slot first when someone else holds it
; (rdx = the element copy function for rt_list_clone)
rt_list_unique proc
    mov rax, qword ptr [rcx]
    test rax, rax
    jz rt_lu_ret
    cmp qword ptr [rax - 16], 1
    jbe rt_lu_ret
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov rcx, rax
    call rt_list_clone
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rcx]
    mov qword ptr [rcx], rax
    mov rcx, rdx
    call rt_release
    mov rsp, rbp
    pop rbp
rt_lu_ret:
    ret
rt_list_unique endp

rt_index_oob proc
    lea rcx, rt_msg_oob
    mov rdx, r9
    jmp rt_abort
rt_index_oob endp

; a list, map or text would hold more than rt_max_len elements (2,147,483,647 bytes for a text)
rt_too_long proc
    lea rcx, rt_msg_toolong
    xor edx, edx
    jmp rt_abort
rt_too_long endp
"#;

/// Removes the `name proc` ... `name endp` block from runtime text.
pub(super) fn strip_proc(rt: &str, name: &str) -> String {
    let start = format!("\n{} proc\n", name);
    let end = format!("\n{} endp\n", name);
    match (rt.find(&start), rt.find(&end)) {
        (Some(a), Some(b)) if b > a => format!("{}{}", &rt[..a], &rt[b + end.len()..]),
        _ => rt.to_string(),
    }
}

/// The built-in heap procedures, left out when the program declares an allocator.
pub(super) fn strip_heap(rt: &str) -> String {
    strip_proc(&strip_proc(rt, "rt_alloc"), "rt_free_obj")
}

/// The built-in heap's data, left out when the program declares an allocator.
pub(super) fn strip_heap_data(data: &str) -> String {
    let mut out = String::new();
    let mut skip_table = false;
    for line in data.lines() {
        let t = line.trim();
        if skip_table {
            skip_table = false;
            if t.starts_with("times") {
                continue;
            }
        }
        if t.starts_with("rt_heap ") || t.starts_with("rt_brk ") || t.starts_with("rt_heap_end ") {
            continue;
        }
        if t == "rt_freelists:" {
            skip_table = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `rt_alloc` and `rt_free_obj` on top of the program's `#[allocator]` functions (and `rt_realloc_obj`
/// when it declares `realloc`). The runtime keeps the header: [total size][refcount][drop], 24 bytes.
pub(super) fn custom_runtime(c: &crate::noalloc::CustomAllocatorInfo) -> String {
    let mut s = RUNTIME_CUSTOM.replace("ALLOC_FN", &c.alloc_sym).replace("FREE_FN", &c.free_sym);
    if let Some(r) = &c.realloc_sym {
        s.push_str(&RUNTIME_CUSTOM_REALLOC.replace("REALLOC_FN", r));
    }
    s
}

pub(super) const RUNTIME_CUSTOM: &str = r#"
; ---- the program's allocator: rt_alloc asks for payload + 24 bytes (rounded up to 16), aligned to 16 ----
; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    mov rcx, rax
    mov edx, 16
    call ALLOC_FN
    test rax, rax
    jz rt_alloc_fail
    mov rcx, qword ptr [rbp - 8]
    lea r9, [rax + 8]
    lea r10, [rax + rcx]
rt_alloc_zero:
    mov qword ptr [r9], 0
    add r9, 8
    cmp r9, r10
    jb rt_alloc_zero
    mov qword ptr [rax], rcx
    mov qword ptr [rax + 8], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 16], rcx
    add rax, 24
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    xor edx, edx
    call rt_abort
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rsp, 40
    sub rcx, 24
    mov rdx, qword ptr [rcx]
    mov r8d, 16
    dec qword ptr rt_live
    call FREE_FN
    add rsp, 40
    ret
rt_free_obj endp
"#;

pub(super) const RUNTIME_CUSTOM_REALLOC: &str = r#"
; rax = the object at payload rcx grown to rdx payload bytes (the new tail is zeroed); its refcount must be 1
rt_realloc_obj proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rdx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    sub rcx, 24
    mov rdx, qword ptr [rcx]
    mov qword ptr [rbp - 16], rdx
    mov r8, rax
    mov r9d, 16
    call REALLOC_FN
    test rax, rax
    jz rt_alloc_fail
    mov rcx, qword ptr [rbp - 8]
    mov rdx, qword ptr [rbp - 16]
    mov qword ptr [rax], rcx
    lea r9, [rax + rdx]
    lea r10, [rax + rcx]
rt_realloc_zero:
    cmp r9, r10
    jae rt_realloc_done
    mov qword ptr [r9], 0
    add r9, 8
    jmp rt_realloc_zero
rt_realloc_done:
    add rax, 24
    mov rsp, rbp
    pop rbp
    ret
rt_realloc_obj endp
"#;

/// In `rt_list_push`: grow the element buffer with the program's `realloc` when it is not shared.
pub(super) const RUNTIME_LIST_REALLOC: &str = "    mov rcx, qword ptr [rbp - 8]
    mov rcx, qword ptr [rcx + 16]
    test rcx, rcx
    jz rt_lp_slow
    cmp qword ptr [rcx - 16], 1
    jne rt_lp_slow
    mov rdx, qword ptr [rbp - 24]
    shl rdx, 3
    call rt_realloc_obj
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rcx + 16], rax
    mov rdx, qword ptr [rbp - 24]
    mov qword ptr [rcx + 8], rdx
    jmp rt_lp_store
rt_lp_slow:
    mov rax, qword ptr [rbp - 24]
";

pub(super) const RUNTIME_MAP: &str = include_str!("../runtime_map.asm");
pub(super) const RUNTIME_DERIVE: &str = include_str!("../runtime_derive.asm");

pub(super) const RUNTIME_WIN: &str = r#"
; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Objects have a 16-byte header: [refcount][drop].
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov rax, qword ptr rt_heap
    test rax, rax
    jnz rt_alloc_have
    call qword ptr GOT_GetProcessHeap
    mov qword ptr rt_heap, rax
rt_alloc_have:
    mov rcx, rax
    mov edx, 8
    mov r8, qword ptr [rbp - 8]
    add r8, 16
    call qword ptr GOT_HeapAlloc
    test rax, rax
    jz rt_alloc_fail
    mov qword ptr [rax], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 8], rcx
    add rax, 16
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    xor edx, edx
    call rt_abort
rt_alloc endp

; write rdx bytes at rcx to stdout
rt_write proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov ecx, -11
    call qword ptr GOT_GetStdHandle
    mov rcx, rax
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    lea r9, [rbp - 24]
    mov qword ptr [rsp + 32], 0
    call qword ptr GOT_WriteFile
    mov rsp, rbp
    pop rbp
    ret
rt_write endp

; write text rcx to stdout; the text is released afterwards when rdx = 1
rt_write_out proc
    sub rsp, 40
    mov rdx, qword ptr [rcx]
    add rcx, 8
    call rt_write
    add rsp, 40
    ret
rt_write_out endp


; write text rcx to standard error
rt_write_err proc
    push rbp
    mov rbp, rsp
    sub rsp, 64
    mov rdx, qword ptr [rcx]
    add rcx, 8
    mov qword ptr [rbp - 8], rcx
    mov qword ptr [rbp - 16], rdx
    mov ecx, -12
    call qword ptr GOT_GetStdHandle
    mov rcx, rax
    mov rdx, qword ptr [rbp - 8]
    mov r8, qword ptr [rbp - 16]
    lea r9, [rbp - 24]
    mov qword ptr [rsp + 32], 0
    call qword ptr GOT_WriteFile
    mov rsp, rbp
    pop rbp
    ret
rt_write_err endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rsp, 40
    mov r8, rcx
    sub r8, 16
    mov rcx, qword ptr rt_heap
    xor edx, edx
    call qword ptr GOT_HeapFree
    dec qword ptr rt_live
    add rsp, 40
    ret
rt_free_obj endp

; exit the process with code ecx
rt_exit proc
    sub rsp, 40
    call qword ptr GOT_ExitProcess
rt_exit endp

rt_div_zero proc
    sub rsp, 8
    lea rcx, rt_msg_divzero
    jmp rt_abort
rt_div_zero endp
"#;

pub(super) const RUNTIME_BARE: &str = r#"
; ---- bare-metal runtime: a size-class heap and a console hook ----

; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Block layout: [total size][refcount][drop][payload...]; freed blocks of
; less than 4096 bytes go on a free list per 16-byte size class.
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    cmp rax, 4096
    jae rt_alloc_bump
    mov rcx, rax
    shr rcx, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rcx * 8]
    test r9, r9
    jz rt_alloc_bump
    mov r10, qword ptr [r9 + 8]
    mov qword ptr [r8 + rcx * 8], r10
    mov rax, r9
    jmp rt_alloc_init
rt_alloc_bump:
    mov rax, qword ptr rt_brk
    mov rcx, qword ptr [rbp - 8]
    lea rdx, [rax + rcx]
    cmp rdx, 67108864
    ja rt_alloc_fail
    mov qword ptr rt_brk, rdx
rt_alloc_init:
    mov rcx, qword ptr [rbp - 8]
    lea r9, [rax + 8]
    lea r10, [rax + rcx]
rt_alloc_zero:
    mov qword ptr [r9], 0
    add r9, 8
    cmp r9, r10
    jb rt_alloc_zero
    mov qword ptr [rax], rcx
    mov qword ptr [rax + 8], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 16], rcx
    add rax, 24
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    xor edx, edx
    call rt_abort
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rcx, 24
    dec qword ptr rt_live
    mov rax, qword ptr [rcx]
    cmp rax, 4096
    jae rt_free_done
    shr rax, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rax * 8]
    mov qword ptr [rcx + 8], r9
    mov qword ptr [r8 + rax * 8], rcx
rt_free_done:
    ret
rt_free_obj endp

; text rcx goes to the kernel's `console_write`, which owns its argument
rt_write_out proc
    sub rsp, 40
    mov qword ptr [rsp + 32], rcx
    call rt_retain
    mov rcx, qword ptr [rsp + 32]
    call B_CONSOLE_WRITE
    add rsp, 40
    ret
rt_write_out endp

; the kernel has one console: an abort message goes there too
rt_write_err proc
    jmp rt_write_out
rt_write_err endp

; stop the machine
rt_exit proc
    cli
rt_exit_loop:
    hlt
    jmp rt_exit_loop
rt_exit endp
"#;

pub(super) const RUNTIME_DATA_WIN: &str = "rt_msg_divzero dq 16
    db \"division by zero\", 0
rt_heap dq 0
";

pub(super) const RUNTIME_DATA_BARE: &str = "rt_brk dq 2097152
rt_freelists:
    times 256 dq 0
";

pub(super) const RUNTIME_DATA_COMMON: &str = r#"rt_live dq 0
rt_kinds dq rt_kind_other_node
rt_kind_other_node dq 0, rt_kind_other_name
dq 4611686018427387904, 0
rt_kind_other_name dq 9
    db "std.Other", 0
dq 4611686018427387904, 0
rt_msg_sep dq 2
    db ": ", 0
dq 4611686018427387904, 0
rt_msg_oom dq 13
    db "out of memory", 0
dq 4611686018427387904, 0
rt_max_len dq 2147483647
rt_msg_toolong dq 29
    db "list, map or text is too long", 0
dq 4611686018427387904, 0
rt_msg_oob dq 19
    db "index out of bounds", 0
dq 4611686018427387904, 0
rt_msg_bytepos dq 14
    db "byte position ", 0
dq 4611686018427387904, 0
rt_msg_textpos dq 14
    db "text position ", 0
dq 4611686018427387904, 0
rt_msg_slicestart dq 12
    db "slice start ", 0
dq 4611686018427387904, 0
rt_msg_outofrange dq 16
    db " is out of range", 0
dq 4611686018427387904, 0
rt_msg_notboundary dq 31
    db " is not on a character boundary", 0
dq 4611686018427387904, 0
rt_msg_afterend dq 23
    db " is after the slice end", 0
dq 4611686018427387904, 0
rt_msg_char1 dq 5
    db "char(", 0
dq 4611686018427387904, 0
rt_msg_char2 dq 26
    db ") is not a valid character", 0
dq 4611686018427387904, 0
rt_msg_take dq 23
    db "take from an empty list", 0
dq 4611686018427387904, 0
rt_msg_nokey dq 17
    db "map key not found", 0
dq 4611686018427387904, 0
rt_msg_floatrange dq 18
    db "float out of range", 0
dq 4611686018427387904, 0
rt_msg_abort dq 7
    db "abort: ", 0
dq 4611686018427387904, 0
rt_msg_error dq 7
    db "error: ", 0
dq 4611686018427387904, 0
rt_msg_nl dq 1
    db 10, 0
"#;

pub(super) const RUNTIME_LINUX: &str = r#"
; ---- Linux runtime: system calls, a size-class heap and mmap for large blocks ----

; rax = zeroed heap object of rcx bytes; rdx = drop procedure (or 0).
; Block layout: [total size][refcount][drop][payload...]. Blocks of less than 4096 bytes come
; from per-size-class free lists and a bump pointer over the zeroed memory after the image;
; larger blocks are their own mmap regions.
rt_alloc proc
    push rbp
    mov rbp, rsp
    sub rsp, 48
    lea rax, [rcx + 39]
    and rax, -16
    mov qword ptr [rbp - 8], rax
    mov qword ptr [rbp - 16], rdx
    cmp rax, 4096
    jae rt_alloc_big
    mov rcx, rax
    shr rcx, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rcx * 8]
    test r9, r9
    jz rt_alloc_bump
    mov r10, qword ptr [r9 + 8]
    mov qword ptr [r8 + rcx * 8], r10
    mov rax, r9
    jmp rt_alloc_init
rt_alloc_bump:
    mov rax, qword ptr rt_brk
    mov rcx, qword ptr [rbp - 8]
    lea rdx, [rax + rcx]
    cmp rdx, qword ptr rt_heap_end
    ja rt_alloc_fail
    mov qword ptr rt_brk, rdx
rt_alloc_init:
    mov rcx, qword ptr [rbp - 8]
    lea r9, [rax + 8]
    lea r10, [rax + rcx]
rt_alloc_zero:
    mov qword ptr [r9], 0
    add r9, 8
    cmp r9, r10
    jb rt_alloc_zero
    jmp rt_alloc_hdr
rt_alloc_big:
    mov qword ptr [rbp - 24], rdi
    mov qword ptr [rbp - 32], rsi
    xor edi, edi
    mov rsi, rax
    mov edx, 3
    mov r10d, 34
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov rdi, qword ptr [rbp - 24]
    mov rsi, qword ptr [rbp - 32]
    cmp rax, -4096
    ja rt_alloc_fail
rt_alloc_hdr:
    mov rcx, qword ptr [rbp - 8]
    mov qword ptr [rax], rcx
    mov qword ptr [rax + 8], 1
    mov rcx, qword ptr [rbp - 16]
    mov qword ptr [rax + 16], rcx
    add rax, 24
    inc qword ptr rt_live
    mov rsp, rbp
    pop rbp
    ret
rt_alloc_fail:
    lea rcx, rt_msg_oom
    xor edx, edx
    call rt_abort
rt_alloc endp

; frees the object whose payload is at rcx
rt_free_obj proc
    sub rcx, 24
    dec qword ptr rt_live
    mov rax, qword ptr [rcx]
    cmp rax, 4096
    jb rt_free_small
    push rdi
    push rsi
    mov rdi, rcx
    mov rsi, rax
    mov eax, 11
    syscall
    pop rsi
    pop rdi
    ret
rt_free_small:
    shr rax, 4
    lea r8, rt_freelists
    mov r9, qword ptr [r8 + rax * 8]
    mov qword ptr [rcx + 8], r9
    mov qword ptr [r8 + rax * 8], rcx
    ret
rt_free_obj endp

; write rdx bytes at rcx to stdout
rt_write proc
    push rdi
    push rsi
    mov rsi, rcx
rt_write_loop:
    test rdx, rdx
    jle rt_write_done
    mov edi, 1
    mov eax, 1
    syscall
    test rax, rax
    jle rt_write_done
    add rsi, rax
    sub rdx, rax
    jmp rt_write_loop
rt_write_done:
    pop rsi
    pop rdi
    ret
rt_write endp

; write text rcx to stdout
rt_write_out proc
    sub rsp, 40
    mov rdx, qword ptr [rcx]
    add rcx, 8
    call rt_write
    add rsp, 40
    ret
rt_write_out endp

; write text rcx to standard error (file descriptor 2)
rt_write_err proc
    push rdi
    push rsi
    mov rdx, qword ptr [rcx]
    lea rsi, [rcx + 8]
    mov edi, 2
    mov eax, 1
    syscall
    pop rsi
    pop rdi
    ret
rt_write_err endp

; exit the process with code ecx
rt_exit proc
    mov edi, ecx
    mov eax, 231
    syscall
rt_exit endp

rt_div_zero proc
    sub rsp, 8
    lea rcx, rt_msg_divzero
    jmp rt_abort
rt_div_zero endp
"#;

pub(super) const RUNTIME_DATA_LINUX: &str = "rt_msg_divzero dq 16
    db \"division by zero\", 0
rt_brk dq 0
rt_heap_end dq 0
rt_freelists:
    times 256 dq 0
";

