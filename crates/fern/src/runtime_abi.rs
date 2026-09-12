//! Canonical external native signatures, independent of Fern source result types.
//! The registry describes 64-bit LP64 targets; pointers, size_t and ssize_t use I64.
//! Runtime declarations are audited against crates/fern-runtime, including
//! callback pointers and native int predicates. libc stdio/math/string and POSIX
//! write retain their system ABI; allocation is owned by the Rust runtime.
//! Registering a physical signature does not enable a source API or its adapters.
//! The unused uint16_t reference-counting APIs stay unregistered until the machine
//! representation can express their narrow ABI and extension requirements.
//! Legacy C-only fern_json_parse/stringify string-copy helpers were removed;
//! JSON uses the typed opaque fern_json_value_* and descriptor codec APIs.
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
        "fern_gc_heap_size"
        | "fern_actor_clock_now"
        | "fern_actor_scheduler_next"
        | "fern_actor_self"
        | "fern_args"
        | "fern_args_count"
        | "fern_cwd"
        | "fern_home"
        | "fern_hostname"
        | "fern_json_value_limit_error"
        | "fern_json_value_null"
        | "fern_list_new"
        | "fern_option_none"
        | "fern_spinner_new"
        | "fern_table_new"
        | "fern_term_color_support"
        | "fern_term_is_tty"
        | "fern_term_size"
        | "fern_user" => (&[], Some(I64), None),
        "fern_gc_collect"
        | "fern_gc_collect_precise"
        | "fern_live_clear_line"
        | "fern_live_done"
        | "fern_term_clear"
        | "fern_term_hide_cursor"
        | "fern_term_restore_cursor"
        | "fern_term_save_cursor"
        | "fern_term_show_cursor" => (&[], None, None),
        "fern_gc_frame_enter" => (&[I64, I64], Some(I64), None),
        "fern_gc_frame_leave" => (&[I64], None, None),
        "fern_float_to_str" | "fern_json_value_from_float" => (&[F64], Some(I64), None),
        "fern_print_float" | "fern_println_float" => (&[F64], None, None),
        "fern_prompt_confirm" | "fern_rc_refcount" => (&[I64], Some(I32), None),
        "fern_actor_clock_advance"
        | "fern_actor_clock_set"
        | "fern_actor_mailbox_len"
        | "fern_actor_next"
        | "fern_actor_receive"
        | "fern_actor_restart"
        | "fern_actor_set_current"
        | "fern_actor_spawn"
        | "fern_actor_spawn_link"
        | "fern_actor_start"
        | "fern_alloc"
        | "fern_arg"
        | "fern_bool_to_str"
        | "fern_chdir"
        | "fern_delete_file"
        | "fern_dup"
        | "fern_exec"
        | "fern_exec_args"
        | "fern_file_exists"
        | "fern_file_size"
        | "fern_getenv"
        | "fern_http_get"
        | "fern_int_to_str"
        | "fern_is_dir"
        | "fern_json_value_as_bool"
        | "fern_json_value_as_float"
        | "fern_json_value_as_int"
        | "fern_json_value_as_string"
        | "fern_json_value_elements"
        | "fern_json_value_error_code"
        | "fern_json_value_error_message"
        | "fern_json_value_error_offset"
        | "fern_json_value_error_path"
        | "fern_json_value_from_array"
        | "fern_json_value_from_bool"
        | "fern_json_value_from_int"
        | "fern_json_value_from_number_text"
        | "fern_json_value_from_string"
        | "fern_json_value_is_null"
        | "fern_json_value_length"
        | "fern_json_value_members"
        | "fern_json_value_number_text"
        | "fern_json_value_parse"
        | "fern_json_value_stringify"
        | "fern_list_dir"
        | "fern_list_head"
        | "fern_list_is_empty"
        | "fern_list_len"
        | "fern_list_reverse"
        | "fern_list_tail"
        | "fern_list_with_capacity"
        | "fern_log_debug"
        | "fern_log_error"
        | "fern_log_info"
        | "fern_log_warn"
        | "fern_managed_fault"
        | "fern_option_is_some"
        | "fern_option_some"
        | "fern_option_unwrap"
        | "fern_panel_new"
        | "fern_panel_render"
        | "fern_progress_advance"
        | "fern_progress_new"
        | "fern_progress_render"
        | "fern_prompt_input"
        | "fern_prompt_password"
        | "fern_rc_dup"
        | "fern_read_dir_result"
        | "fern_read_file"
        | "fern_result_err"
        | "fern_result_is_ok"
        | "fern_result_ok"
        | "fern_result_unwrap"
        | "fern_spinner_render"
        | "fern_spinner_tick"
        | "fern_sql_open"
        | "fern_sql_close"
        | "fern_status_debug"
        | "fern_status_error"
        | "fern_status_info"
        | "fern_status_ok"
        | "fern_status_warn"
        | "fern_str_decimal_size_is_valid"
        | "fern_str_is_decimal"
        | "fern_str_is_empty"
        | "fern_str_len"
        | "fern_str_lines"
        | "fern_str_to_lower"
        | "fern_str_to_upper"
        | "fern_str_trim"
        | "fern_str_trim_end"
        | "fern_str_trim_start"
        | "fern_style_black"
        | "fern_style_blink"
        | "fern_style_blue"
        | "fern_style_bold"
        | "fern_style_bright_black"
        | "fern_style_bright_blue"
        | "fern_style_bright_cyan"
        | "fern_style_bright_green"
        | "fern_style_bright_magenta"
        | "fern_style_bright_red"
        | "fern_style_bright_white"
        | "fern_style_bright_yellow"
        | "fern_style_cyan"
        | "fern_style_dim"
        | "fern_style_green"
        | "fern_style_italic"
        | "fern_style_magenta"
        | "fern_style_on_black"
        | "fern_style_on_blue"
        | "fern_style_on_cyan"
        | "fern_style_on_green"
        | "fern_style_on_magenta"
        | "fern_style_on_red"
        | "fern_style_on_white"
        | "fern_style_on_yellow"
        | "fern_style_red"
        | "fern_style_reset"
        | "fern_style_reverse"
        | "fern_style_strikethrough"
        | "fern_style_underline"
        | "fern_style_white"
        | "fern_style_yellow"
        | "fern_table_render"
        | "fern_tree_new"
        | "fern_tree_render"
        | "fern_write_stderr" => (&[I64], Some(I64), None),
        "fern_drop"
        | "fern_exit"
        | "fern_free"
        | "fern_list_free"
        | "fern_live_print"
        | "fern_live_update"
        | "fern_managed_run"
        | "fern_managed_stop"
        | "fern_panel_free"
        | "fern_print_bool"
        | "fern_print_int"
        | "fern_print_str"
        | "fern_println_bool"
        | "fern_println_int"
        | "fern_println_str"
        | "fern_progress_free"
        | "fern_rc_drop"
        | "fern_regex_captures_free"
        | "fern_regex_match_free"
        | "fern_sleep_ms"
        | "fern_spinner_free"
        | "fern_str_list_free"
        | "fern_table_free"
        | "fern_term_down"
        | "fern_term_left"
        | "fern_term_right"
        | "fern_term_up" => (&[I64], None, None),
        "pow" => (&[F64, F64], Some(F64), None),
        "fern_set_args" => (&[I32, I64], None, None),
        "fern_prompt_select" => (&[I64, I64], Some(I32), None),
        "fern_actor_demonitor"
        | "fern_actor_exit"
        | "fern_actor_monitor"
        | "fern_actor_post"
        | "fern_actor_send"
        | "fern_append_file"
        | "fern_http_post"
        | "fern_json_codec_decode"
        | "fern_json_codec_encode"
        | "fern_json_value_at"
        | "fern_json_value_from_object"
        | "fern_json_value_get"
        | "fern_list_all"
        | "fern_list_any"
        | "fern_list_concat"
        | "fern_list_contains"
        | "fern_list_contains_str"
        | "fern_list_filter"
        | "fern_list_find"
        | "fern_list_get"
        | "fern_list_map"
        | "fern_list_push"
        | "fern_managed_continue"
        | "fern_option_map"
        | "fern_option_unwrap_or"
        | "fern_panel_border"
        | "fern_panel_border_color"
        | "fern_panel_border_str"
        | "fern_panel_subtitle"
        | "fern_panel_title"
        | "fern_panel_width"
        | "fern_progress_description"
        | "fern_progress_set"
        | "fern_progress_width"
        | "fern_regex_captures"
        | "fern_regex_find"
        | "fern_regex_find_all"
        | "fern_regex_is_match"
        | "fern_regex_split"
        | "fern_result_and_then"
        | "fern_result_map"
        | "fern_result_unwrap_or"
        | "fern_result_unwrap_or_else"
        | "fern_setenv"
        | "fern_spinner_message"
        | "fern_spinner_style"
        | "fern_sql_execute"
        | "fern_str_char_at"
        | "fern_str_concat"
        | "fern_str_contains"
        | "fern_str_ends_with"
        | "fern_str_eq"
        | "fern_str_index_of"
        | "fern_str_join"
        | "fern_str_repeat"
        | "fern_str_split"
        | "fern_str_split_is_valid"
        | "fern_str_starts_with"
        | "fern_style_color"
        | "fern_style_hex"
        | "fern_style_on_color"
        | "fern_style_on_hex"
        | "fern_table_add_column"
        | "fern_table_add_row"
        | "fern_table_border"
        | "fern_table_show_header"
        | "fern_table_title"
        | "fern_tree_add"
        | "fern_write_file"
        | "strstr" => (&[I64, I64], Some(I64), None),
        "fern_list_push_mut" | "fern_term_move_to" => (&[I64, I64], None, None),
        "write" => (&[I32, I64, I64], Some(I64), None),
        "fern_exec_args_bounded"
        | "fern_list_fold"
        | "fern_managed_new"
        | "fern_managed_spawn"
        | "fern_panel_padding"
        | "fern_prompt_int"
        | "fern_regex_replace"
        | "fern_regex_replace_all"
        | "fern_str_replace"
        | "fern_str_slice"
        | "fern_str_slice_is_valid" => (&[I64, I64, I64], Some(I64), None),
        "fern_actor_supervise"
        | "fern_actor_supervise_one_for_all"
        | "fern_actor_supervise_rest_for_one"
        | "fern_managed_receive"
        | "fern_managed_send"
        | "fern_style_on_rgb"
        | "fern_style_rgb" => (&[I64, I64, I64, I64], Some(I64), None),
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
            if token.starts_with("fern_")
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
            "fern_str_eq",
            "fern_result_is_ok",
            "fern_list_is_empty",
            "fern_json_value_is_null",
        ] {
            assert_eq!(signature(symbol).unwrap().result, Some(I64), "{symbol}");
        }
        assert_eq!(signature("fern_print_bool").unwrap().params, [I64]);
        assert_eq!(signature("fern_prompt_confirm").unwrap().result, Some(I32));
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
        for symbol in ["fern_print_float", "fern_println_float"] {
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
            signature("fern_float_to_str").unwrap(),
            Signature {
                params: vec![F64],
                result: Some(I64),
                variadic: None
            }
        );
        assert_eq!(
            signature("fern_json_value_from_float").unwrap().params,
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
            signature("fern_managed_new").unwrap().params,
            [I64, I64, I64]
        );
        assert_eq!(
            signature("fern_managed_send").unwrap().params,
            [I64, I64, I64, I64]
        );
        assert_eq!(signature("fern_managed_stop").unwrap().result, None);
        assert_eq!(
            signature("fern_alloc").unwrap(),
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
            "fern_rs_report_fault",
            "fern_rc_alloc",
            "fern_rc_flags",
            "fern_str_eq_typo",
            "fern_json_parse",
            "fern_json_stringify",
        ] {
            assert_eq!(signature(symbol), None, "{symbol}");
        }
    }
}
