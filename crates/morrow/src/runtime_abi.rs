//! Canonical external native signatures, independent of Morrow source result types.
//! The registry describes 64-bit LP64 targets; pointers, size_t and ssize_t use I64.
//! Runtime declarations are audited against crates/morrow-runtime, including
//! callback pointers and native int predicates. libc stdio/math/string and POSIX
//! write retain their system ABI; allocation is owned by the Rust runtime.
//! Registering a physical signature does not enable a source API or its adapters.
//! The unused uint16_t reference-counting APIs stay unregistered until the machine
//! representation can express their narrow ABI and extension requirements.
//! Legacy C-only morrow_json_parse/stringify string-copy helpers were removed;
//! JSON uses the typed opaque morrow_json_value_* and descriptor codec APIs.
use crate::machine::Scalar;

/// Exact C declaration transport, including the result even when a caller discards it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub params: Vec<Scalar>,
    pub result: Option<Scalar>,
    /// Number of fixed arguments for a variadic declaration; never backend permission.
    pub variadic: Option<usize>,
}

/// Resolve only audited external symbols; internal functions belong to the machine program.
pub fn signature(symbol: &str) -> Option<Signature> {
    use Scalar::{F64, I32, I64};
    let (params, result, variadic): (&[Scalar], Option<Scalar>, Option<usize>) = match symbol {
        // ISO C stdio declarations: these must be rejected by backends without
        // audited target-specific variadic lowering, even for discarded results.
        "printf" => (&[I64], Some(I32), Some(1)),
        "snprintf" => (&[I64, I64, I64], Some(I32), Some(3)),
        "morrow_gc_heap_size"
        | "morrow_actor_clock_now"
        | "morrow_actor_scheduler_next"
        | "morrow_actor_self"
        | "morrow_args"
        | "morrow_args_count"
        | "morrow_cwd"
        | "morrow_home"
        | "morrow_hostname"
        | "morrow_json_value_limit_error"
        | "morrow_json_value_null"
        | "morrow_list_new"
        | "morrow_option_none"
        | "morrow_spinner_new"
        | "morrow_table_new"
        | "morrow_term_color_support"
        | "morrow_term_is_tty"
        | "morrow_term_size"
        | "morrow_user" => (&[], Some(I64), None),
        "morrow_gc_collect"
        | "morrow_gc_collect_precise"
        | "morrow_managed_pin_current"
        | "morrow_live_clear_line"
        | "morrow_live_done"
        | "morrow_term_clear"
        | "morrow_term_hide_cursor"
        | "morrow_term_restore_cursor"
        | "morrow_term_save_cursor"
        | "morrow_term_show_cursor" => (&[], None, None),
        "morrow_ffi_float32" => (&[F64], Some(I64), None),
        "morrow_ffi_borrow_string" => (&[I64], Some(I64), None),
        "morrow_ffi_read_string" => (&[I64, I64], Some(I64), None),
        "morrow_gc_frame_enter" => (&[I64, I64], Some(I64), None),
        "morrow_gc_frame_leave" => (&[I64], None, None),
        "morrow_float_to_str" | "morrow_json_value_from_float" => (&[F64], Some(I64), None),
        "morrow_print_float" | "morrow_println_float" => (&[F64], None, None),
        "morrow_prompt_confirm" | "morrow_rc_refcount" => (&[I64], Some(I32), None),
        "morrow_actor_clock_advance"
        | "morrow_actor_clock_set"
        | "morrow_actor_mailbox_len"
        | "morrow_actor_next"
        | "morrow_actor_receive"
        | "morrow_actor_restart"
        | "morrow_actor_set_current"
        | "morrow_actor_spawn"
        | "morrow_actor_spawn_link"
        | "morrow_actor_start"
        | "morrow_alloc"
        | "morrow_arg"
        | "morrow_bool_to_str"
        | "morrow_chdir"
        | "morrow_delete_file"
        | "morrow_dup"
        | "morrow_exec"
        | "morrow_exec_args"
        | "morrow_file_exists"
        | "morrow_file_size"
        | "morrow_getenv"
        | "morrow_http_get"
        | "morrow_int_to_str"
        | "morrow_is_dir"
        | "morrow_json_value_as_bool"
        | "morrow_json_value_as_float"
        | "morrow_json_value_as_int"
        | "morrow_json_value_as_string"
        | "morrow_json_value_elements"
        | "morrow_json_value_error_code"
        | "morrow_json_value_error_message"
        | "morrow_json_value_error_offset"
        | "morrow_json_value_error_path"
        | "morrow_json_value_from_array"
        | "morrow_json_value_from_bool"
        | "morrow_json_value_from_int"
        | "morrow_json_value_from_number_text"
        | "morrow_json_value_from_string"
        | "morrow_json_value_is_null"
        | "morrow_json_value_length"
        | "morrow_json_value_members"
        | "morrow_json_value_number_text"
        | "morrow_json_value_parse"
        | "morrow_json_value_stringify"
        | "morrow_int_checked_neg"
        | "morrow_int_parse"
        | "morrow_list_dir"
        | "morrow_list_first"
        | "morrow_list_head"
        | "morrow_list_is_empty"
        | "morrow_list_last"
        | "morrow_list_sort"
        | "morrow_list_sort_float"
        | "morrow_list_sort_str"
        | "morrow_list_sum"
        | "morrow_list_len"
        | "morrow_list_reverse"
        | "morrow_list_tail"
        | "morrow_list_with_capacity"
        | "morrow_log_debug"
        | "morrow_log_error"
        | "morrow_log_info"
        | "morrow_log_warn"
        | "morrow_managed_fault"
        | "morrow_managed_scope_enter"
        | "morrow_managed_scope_leave"
        | "morrow_tui_fault_enter"
        | "morrow_option_is_some"
        | "morrow_option_some"
        | "morrow_option_unwrap"
        | "morrow_panel_new"
        | "morrow_panel_render"
        | "morrow_progress_advance"
        | "morrow_progress_new"
        | "morrow_progress_render"
        | "morrow_prompt_input"
        | "morrow_prompt_password"
        | "morrow_rc_dup"
        | "morrow_read_dir_result"
        | "morrow_read_file"
        | "morrow_result_err"
        | "morrow_result_is_ok"
        | "morrow_result_ok"
        | "morrow_result_unwrap"
        | "morrow_spinner_render"
        | "morrow_spinner_tick"
        | "morrow_sql_open"
        | "morrow_sql_close"
        | "morrow_status_debug"
        | "morrow_status_error"
        | "morrow_status_info"
        | "morrow_status_ok"
        | "morrow_status_warn"
        | "morrow_str_decimal_size_is_valid"
        | "morrow_str_is_decimal"
        | "morrow_str_is_empty"
        | "morrow_str_len"
        | "morrow_str_lines"
        | "morrow_str_to_lower"
        | "morrow_str_quote"
        | "morrow_str_to_upper"
        | "morrow_str_trim"
        | "morrow_str_trim_end"
        | "morrow_str_trim_start"
        | "morrow_style_black"
        | "morrow_style_blink"
        | "morrow_style_blue"
        | "morrow_style_bold"
        | "morrow_style_bright_black"
        | "morrow_style_bright_blue"
        | "morrow_style_bright_cyan"
        | "morrow_style_bright_green"
        | "morrow_style_bright_magenta"
        | "morrow_style_bright_red"
        | "morrow_style_bright_white"
        | "morrow_style_bright_yellow"
        | "morrow_style_cyan"
        | "morrow_style_dim"
        | "morrow_style_green"
        | "morrow_style_italic"
        | "morrow_style_magenta"
        | "morrow_style_on_black"
        | "morrow_style_on_blue"
        | "morrow_style_on_cyan"
        | "morrow_style_on_green"
        | "morrow_style_on_magenta"
        | "morrow_style_on_red"
        | "morrow_style_on_white"
        | "morrow_style_on_yellow"
        | "morrow_style_red"
        | "morrow_style_reset"
        | "morrow_style_reverse"
        | "morrow_style_strikethrough"
        | "morrow_style_underline"
        | "morrow_style_white"
        | "morrow_style_yellow"
        | "morrow_table_render"
        | "morrow_tree_new"
        | "morrow_tree_render"
        | "morrow_write_stderr" => (&[I64], Some(I64), None),
        "morrow_drop"
        | "morrow_exit"
        | "morrow_free"
        | "morrow_list_free"
        | "morrow_live_print"
        | "morrow_live_update"
        | "morrow_managed_run"
        | "morrow_managed_stop"
        | "morrow_managed_close"
        | "morrow_tui_fault_leave"
        | "morrow_panel_free"
        | "morrow_print_bool"
        | "morrow_print_int"
        | "morrow_print_str"
        | "morrow_println_bool"
        | "morrow_println_int"
        | "morrow_println_str"
        | "morrow_progress_free"
        | "morrow_rc_drop"
        | "morrow_regex_captures_free"
        | "morrow_regex_match_free"
        | "morrow_sleep_ms"
        | "morrow_spinner_free"
        | "morrow_str_list_free"
        | "morrow_table_free"
        | "morrow_term_down"
        | "morrow_term_left"
        | "morrow_term_right"
        | "morrow_term_up" => (&[I64], None, None),
        "pow" => (&[F64, F64], Some(F64), None),
        "morrow_json_codec_scope_check" => (&[I64], None, None),
        "morrow_json_codec_encode_context" | "morrow_json_codec_decode_context" => {
            (&[I64, I64, I64], Some(I64), None)
        }
        "morrow_set_args" => (&[I32, I64], None, None),
        "morrow_prompt_select" => (&[I64, I64], Some(I32), None),
        "morrow_actor_demonitor"
        | "morrow_actor_exit"
        | "morrow_actor_monitor"
        | "morrow_actor_post"
        | "morrow_actor_send"
        | "morrow_append_file"
        | "morrow_http_post"
        | "morrow_json_codec_decode"
        | "morrow_json_codec_encode"
        | "morrow_json_value_at"
        | "morrow_json_value_from_object"
        | "morrow_json_value_get"
        | "morrow_int_checked_add"
        | "morrow_int_checked_div"
        | "morrow_int_checked_mul"
        | "morrow_int_checked_rem"
        | "morrow_int_checked_sub"
        | "morrow_list_all"
        | "morrow_list_any"
        | "morrow_list_at"
        | "morrow_list_concat"
        | "morrow_list_contains"
        | "morrow_list_contains_str"
        | "morrow_list_drop"
        | "morrow_list_filter"
        | "morrow_list_find"
        | "morrow_list_get"
        | "morrow_list_range"
        | "morrow_list_take"
        | "morrow_list_zip"
        | "morrow_list_map"
        | "morrow_list_push"
        | "morrow_managed_continue"
        | "morrow_managed_scope_defer"
        | "morrow_managed_supervised_current"
        | "morrow_managed_poll"
        | "morrow_managed_parallel"
        | "morrow_managed_port"
        | "morrow_managed_port_peek_len"
        | "morrow_option_map"
        | "morrow_option_unwrap_or"
        | "morrow_panel_border"
        | "morrow_panel_border_color"
        | "morrow_panel_border_str"
        | "morrow_panel_subtitle"
        | "morrow_panel_title"
        | "morrow_panel_width"
        | "morrow_progress_description"
        | "morrow_progress_set"
        | "morrow_progress_width"
        | "morrow_regex_captures"
        | "morrow_regex_find"
        | "morrow_regex_find_all"
        | "morrow_regex_is_match"
        | "morrow_regex_split"
        | "morrow_result_and_then"
        | "morrow_result_map"
        | "morrow_result_unwrap_or"
        | "morrow_result_unwrap_or_else"
        | "morrow_setenv"
        | "morrow_spinner_message"
        | "morrow_spinner_style"
        | "morrow_sql_execute"
        | "morrow_str_char_at"
        | "morrow_str_concat"
        | "morrow_str_contains"
        | "morrow_str_ends_with"
        | "morrow_str_eq"
        | "morrow_str_compare"
        | "morrow_str_index_of"
        | "morrow_str_join"
        | "morrow_str_repeat"
        | "morrow_str_split"
        | "morrow_str_split_is_valid"
        | "morrow_str_starts_with"
        | "morrow_style_color"
        | "morrow_style_hex"
        | "morrow_style_on_color"
        | "morrow_style_on_hex"
        | "morrow_table_add_column"
        | "morrow_table_add_row"
        | "morrow_table_border"
        | "morrow_table_show_header"
        | "morrow_table_title"
        | "morrow_tree_add"
        | "morrow_write_file"
        | "strstr" => (&[I64, I64], Some(I64), None),
        "morrow_list_push_mut" | "morrow_sort_report" | "morrow_term_move_to" => {
            (&[I64, I64], None, None)
        }
        "morrow_sort_begin" | "morrow_sort_next" => (&[I64], Some(I64), None),
        "morrow_sort_finish" => (&[I64, I64], Some(I64), None),
        "write" => (&[I32, I64, I64], Some(I64), None),
        "morrow_exec_args_bounded"
        | "morrow_list_fold"
        | "morrow_managed_new"
        | "morrow_managed_open"
        | "morrow_managed_spawn"
        | "morrow_panel_padding"
        | "morrow_prompt_int"
        | "morrow_regex_replace"
        | "morrow_regex_replace_all"
        | "morrow_str_replace"
        | "morrow_str_slice"
        | "morrow_str_slice_is_valid" => (&[I64, I64, I64], Some(I64), None),
        "morrow_actor_supervise"
        | "morrow_actor_supervise_one_for_all"
        | "morrow_actor_supervise_rest_for_one"
        | "morrow_managed_receive"
        | "morrow_managed_send"
        | "morrow_managed_supervise"
        | "morrow_managed_spawn_on"
        | "morrow_managed_port_read"
        | "morrow_regex_replace_checked"
        | "morrow_regex_replace_all_checked"
        | "morrow_style_on_rgb"
        | "morrow_style_rgb" => (&[I64, I64, I64, I64], Some(I64), None),
        _ => return None,
    };
    Some(Signature {
        params: params.to_vec(),
        result,
        variadic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Scalar::{F64, I32, I64};

    #[test]
    fn source_runtime_entries_have_explicit_external_signatures() {
        // Keep this oracle independent of ValueAbi: its Bool/Unit transport can
        // intentionally differ from the actual C declaration.
        let source = include_str!("runtime.rs");
        let (_, entries) = source.split_once("const ENTRIES:").unwrap();
        let (entries, _) = entries.split_once("const OMISSIONS:").unwrap();
        let mut count = 0;
        for token in entries.split('"') {
            if token.starts_with("morrow_")
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                assert!(
                    signature(token).is_some(),
                    "missing C declaration for {token}"
                );
                count += 1;
            }
        }
        assert!(
            count >= 190,
            "source registry extraction lost its inventory"
        );
    }

    #[test]
    fn predicates_preserve_c_int64_results_despite_source_bool() {
        for symbol in [
            "morrow_str_eq",
            "morrow_result_is_ok",
            "morrow_list_is_empty",
            "morrow_json_value_is_null",
        ] {
            assert_eq!(signature(symbol).unwrap().result, Some(I64), "{symbol}");
        }
        assert_eq!(signature("morrow_print_bool").unwrap().params, [I64]);
        assert_eq!(
            signature("morrow_prompt_confirm").unwrap().result,
            Some(I32)
        );
    }

    #[test]
    fn libc_retains_discarded_results_and_fixed_vararg_prefix() {
        assert_eq!(
            signature("write").unwrap(),
            Signature {
                params: vec![I32, I64, I64],
                result: Some(I64),
                variadic: None
            }
        );
        assert_eq!(
            signature("printf").unwrap(),
            Signature {
                params: vec![I64],
                result: Some(I32),
                variadic: Some(1)
            }
        );
        assert_eq!(
            signature("snprintf").unwrap(),
            Signature {
                params: vec![I64, I64, I64],
                result: Some(I32),
                variadic: Some(3)
            }
        );
    }

    #[test]
    fn float_wrappers_and_json_use_real_double_abi() {
        for symbol in ["morrow_print_float", "morrow_println_float"] {
            assert_eq!(
                signature(symbol).unwrap(),
                Signature {
                    params: vec![F64],
                    result: None,
                    variadic: None
                }
            );
        }
        assert_eq!(
            signature("morrow_float_to_str").unwrap(),
            Signature {
                params: vec![F64],
                result: Some(I64),
                variadic: None
            }
        );
        assert_eq!(
            signature("morrow_json_value_from_float").unwrap().params,
            [F64]
        );
        assert_eq!(
            signature("pow").unwrap(),
            Signature {
                params: vec![F64, F64],
                result: Some(F64),
                variadic: None
            }
        );
    }

    #[test]
    fn actor_abi_and_allocation_preserve_pointer_widths() {
        assert_eq!(
            signature("morrow_managed_new").unwrap().params,
            [I64, I64, I64]
        );
        assert_eq!(
            signature("morrow_managed_send").unwrap().params,
            [I64, I64, I64, I64]
        );
        assert_eq!(signature("morrow_managed_stop").unwrap().result, None);
        assert_eq!(
            signature("morrow_managed_parallel").unwrap().params,
            [I64, I64]
        );
        assert_eq!(
            signature("morrow_managed_spawn_on").unwrap().params,
            [I64, I64, I64, I64]
        );
        assert_eq!(
            signature("morrow_alloc").unwrap(),
            Signature {
                params: vec![I64],
                result: Some(I64),
                variadic: None
            }
        );
    }

    #[test]
    fn unknown_internal_and_unsupported_narrow_abis_are_rejected() {
        for symbol in [
            "unknown",
            "f0",
            "morrow_rs_report_fault",
            "morrow_rc_alloc",
            "morrow_rc_flags",
            "morrow_str_eq_typo",
            "morrow_json_parse",
            "morrow_json_stringify",
        ] {
            assert_eq!(signature(symbol), None, "{symbol}");
        }
    }
}
