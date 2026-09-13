use super::{abi, display_width, rendered, repeat, text};
use std::ffi::c_char;

const BOXES: [[&str; 8]; 6] = [
    ["╭", "─", "╮", "│", "│", "╰", "─", "╯"],
    ["┌", "─", "┐", "│", "│", "└", "─", "┘"],
    ["╔", "═", "╗", "║", "║", "╚", "═", "╝"],
    ["┏", "━", "┓", "┃", "┃", "┗", "━", "┛"],
    ["+", "-", "+", "|", "|", "+", "-", "+"],
    [" ", " ", " ", " ", " ", " ", " ", " "],
];

fn layout_limit(width: usize, lines: usize) -> bool {
    if width
        .checked_mul(lines)
        .and_then(|n| n.checked_mul(4))
        .is_none_or(|n| n > crate::io::TEXT_LIMIT)
    {
        super::limit_fault("terminal rendering exceeds 16 MiB");
        return false;
    }
    true
}

fn padded(value: &str, width: usize, center: bool) -> String {
    let count = width.saturating_sub(display_width(value));
    let left = if center { count / 2 } else { 0 };
    format!("{}{value}{}", repeat(" ", left), repeat(" ", count - left))
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Panel {
    content: *const c_char,
    title: *const c_char,
    subtitle: *const c_char,
    border_color: *const c_char,
    box_style: i32,
    width: i64,
    padding_h: i64,
    padding_v: i64,
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_new(content: *const c_char) -> *mut Panel {
    abi::owned(
        Panel {
            content: abi::string(unsafe { text(content) }),
            title: std::ptr::null(),
            subtitle: std::ptr::null(),
            border_color: std::ptr::null(),
            box_style: 0,
            width: 0,
            padding_h: 1,
            padding_v: 0,
        },
        0,
    )
}

macro_rules! string_setter {
    ($name:ident, $kind:ty, $field:ident) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Non-null string pointers must reference readable NUL-terminated storage.
        /// Object and list pointers must use the declared runtime ABI, remain live for
        /// this call, and allow any requested mutation without aliasing or concurrent access.
        pub unsafe extern "C" fn $name(pointer: *mut $kind, value: *const c_char) -> *mut $kind {
            let roots = [pointer as usize, value as usize];
            let _root = unsafe { crate::memory::root_range(roots.as_ptr(), 2) };
            if let Some(target) = unsafe { pointer.as_mut() } {
                target.$field = if value.is_null() {
                    std::ptr::null()
                } else {
                    abi::string(unsafe { text(value) })
                };
            }
            pointer
        }
    };
}
string_setter!(fern_panel_title, Panel, title);
string_setter!(fern_panel_subtitle, Panel, subtitle);
string_setter!(fern_panel_border_color, Panel, border_color);

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_border(panel: *mut Panel, style: i64) -> *mut Panel {
    if let Some(panel) = unsafe { panel.as_mut() }
        && (0..=5).contains(&style)
    {
        panel.box_style = style as i32;
    }
    panel
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_border_str(
    panel: *mut Panel,
    style: *const c_char,
) -> *mut Panel {
    if let Some(style) = ["rounded", "square", "double", "heavy", "ascii", "none"]
        .iter()
        .position(|name| *name == unsafe { text(style) })
    {
        unsafe {
            fern_panel_border(panel, style as i64);
        }
    }
    panel
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_width(panel: *mut Panel, width: i64) -> *mut Panel {
    if let Some(panel) = unsafe { panel.as_mut() } {
        panel.width = width;
    }
    panel
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_padding(
    panel: *mut Panel,
    vertical: i64,
    horizontal: i64,
) -> *mut Panel {
    if let Some(panel) = unsafe { panel.as_mut() } {
        panel.padding_v = vertical.max(0);
        panel.padding_h = horizontal.max(0);
    }
    panel
}

fn panel_border(
    boxes: &[&str; 8],
    title: &str,
    width: usize,
    bottom: bool,
    start: &str,
    end: &str,
) -> String {
    let (left, line, right) = if bottom {
        (boxes[5], boxes[6], boxes[7])
    } else {
        (boxes[0], boxes[1], boxes[2])
    };
    let body = if title.is_empty() {
        repeat(line, width)
    } else {
        let sides = width.saturating_sub(display_width(title) + 2);
        format!(
            "{}{end} {title} {start}{}",
            repeat(line, sides / 2),
            repeat(line, sides - sides / 2)
        )
    };
    format!("{start}{left}{body}{right}{end}")
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_panel_render(panel: *const Panel) -> *const c_char {
    let Some(panel) = (unsafe { panel.as_ref() }) else {
        return abi::string("");
    };
    let content = unsafe { text(panel.content) };
    let title = unsafe { text(panel.title) };
    let subtitle = unsafe { text(panel.subtitle) };
    let border = super::style::named_color(unsafe { text(panel.border_color) });
    let reset = if border.is_empty() { "" } else { "\x1b[0m" };
    let boxes = &BOXES[panel.box_style.clamp(0, 5) as usize];
    let horizontal = panel.padding_h.max(0) as usize;
    let vertical = panel.padding_v.max(0) as usize;
    let titles = if title.is_empty() {
        0
    } else {
        display_width(title) + 2
    }
    .max(if subtitle.is_empty() {
        0
    } else {
        display_width(subtitle) + 2
    });
    let natural = content
        .split('\n')
        .map(display_width)
        .max()
        .unwrap_or(0)
        .max(titles)
        .saturating_add(horizontal.saturating_mul(2));
    let requested = if panel.width == -1 {
        super::terminal::dimensions().cols
    } else {
        panel.width
    };
    let width = if requested > 0 {
        (requested as usize)
            .saturating_sub(2)
            .max(titles)
            .max(horizontal.saturating_mul(2))
    } else {
        natural
    };
    let lines: Vec<&str> = if content.is_empty() {
        vec![""]
    } else {
        content.split_terminator('\n').collect()
    };
    if !layout_limit(
        width.saturating_add(2),
        lines
            .len()
            .saturating_add(vertical.saturating_mul(2))
            .saturating_add(2),
    ) {
        return std::ptr::null();
    }
    let mut result = panel_border(boxes, title, width, false, &border, reset);
    let padding = repeat(" ", horizontal);
    let blank = repeat(" ", width);
    let empty_line = format!(
        "\n{border}{}{reset}{blank}{border}{}{reset}",
        boxes[3], boxes[4]
    );
    for _ in 0..vertical {
        result.push_str(&empty_line);
    }
    for line in lines {
        result.push_str(&format!(
            "\n{border}{}{reset}{padding}{}{padding}{border}{}{reset}",
            boxes[3],
            padded(line, width.saturating_sub(2 * horizontal), false),
            boxes[4]
        ));
    }
    for _ in 0..vertical {
        result.push_str(&empty_line);
    }
    result.push('\n');
    result.push_str(&panel_border(boxes, subtitle, width, true, &border, reset));
    rendered(&result)
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Column {
    header: *const c_char,
    min_width: i64,
    max_width: i64,
    justify: i64,
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Row {
    cells: *mut *const c_char,
    cell_count: i64,
}
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Table {
    title: *const c_char,
    caption: *const c_char,
    columns: *mut Column,
    column_count: i64,
    rows: *mut Row,
    row_count: i64,
    row_capacity: i64,
    box_style: i32,
    show_header: i64,
    show_lines: i64,
    expand: i64,
}

#[unsafe(no_mangle)]
pub extern "C" fn fern_table_new() -> *mut Table {
    abi::owned(
        Table {
            title: std::ptr::null(),
            caption: std::ptr::null(),
            columns: std::ptr::null_mut(),
            column_count: 0,
            rows: std::ptr::null_mut(),
            row_count: 0,
            row_capacity: 0,
            box_style: 0,
            show_header: 1,
            show_lines: 0,
            expand: 0,
        },
        0,
    )
}

unsafe fn array<T: Copy>(values: &[T]) -> *mut T {
    let size = std::mem::size_of_val(values);
    if size > crate::io::TEXT_LIMIT {
        super::limit_fault("terminal collection exceeds 16 MiB");
        return std::ptr::null_mut();
    }
    let _root =
        unsafe { crate::memory::root_range(values.as_ptr().cast(), size / size_of::<usize>()) };
    let pointer = crate::memory::alloc(size.max(1), false).cast::<T>();
    unsafe {
        std::ptr::copy_nonoverlapping(values.as_ptr(), pointer, values.len());
    }
    pointer
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_table_add_column(
    table: *mut Table,
    header: *const c_char,
) -> *mut Table {
    let roots = [table as usize, header as usize];
    let _root = unsafe { crate::memory::root_range(roots.as_ptr(), 2) };
    let Some(target) = (unsafe { table.as_mut() }) else {
        return table;
    };
    let mut columns = if target.column_count == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(target.columns, target.column_count as usize) }.to_vec()
    };
    columns.push(Column {
        header: abi::string(unsafe { text(header) }),
        min_width: 0,
        max_width: 0,
        justify: 0,
    });
    let copied = unsafe { array(&columns) };
    if copied.is_null() {
        return std::ptr::null_mut();
    }
    target.columns = copied;
    target.column_count = columns.len() as i64;
    table
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_table_add_row(
    table: *mut Table,
    cells: *const abi::StringList,
) -> *mut Table {
    let roots = [table as usize, cells as usize];
    let _root = unsafe { crate::memory::root_range(roots.as_ptr(), 2) };
    let (Some(target), Some(cells)) = (unsafe { table.as_mut() }, unsafe { cells.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if cells.len < 0 || cells.len > cells.cap || (cells.len != 0 && cells.data.is_null()) {
        return std::ptr::null_mut();
    }
    let texts: Vec<&str> = (0..cells.len as usize)
        .map(|i| unsafe { text(*cells.data.add(i)) })
        .collect();
    let copies = abi::strings(&texts);
    let words = [copies as usize];
    let _copies = unsafe { crate::memory::root_range(words.as_ptr(), 1) };
    let mut rows = if target.row_count == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(target.rows, target.row_count as usize) }.to_vec()
    };
    rows.push(Row {
        cells: unsafe { (*copies).data },
        cell_count: cells.len,
    });
    let copied = unsafe { array(&rows) };
    if copied.is_null() {
        return std::ptr::null_mut();
    }
    target.rows = copied;
    target.row_count = rows.len() as i64;
    target.row_capacity = target.row_count;
    table
}
string_setter!(fern_table_title, Table, title);
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_table_border(table: *mut Table, style: i64) -> *mut Table {
    if let Some(table) = unsafe { table.as_mut() }
        && (0..=5).contains(&style)
    {
        table.box_style = style as i32;
    }
    table
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_table_show_header(table: *mut Table, show: i64) -> *mut Table {
    if let Some(table) = unsafe { table.as_mut() } {
        table.show_header = (show != 0) as i64;
    }
    table
}

#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_table_render(table: *const Table) -> *const c_char {
    let Some(table) = (unsafe { table.as_ref() }) else {
        return abi::string("");
    };
    if table.column_count <= 0 {
        return abi::string("");
    }
    let columns = unsafe { std::slice::from_raw_parts(table.columns, table.column_count as usize) };
    let rows = if table.row_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(table.rows, table.row_count as usize) }
    };
    let mut widths: Vec<usize> = columns
        .iter()
        .map(|column| display_width(unsafe { text(column.header) }))
        .collect();
    for row in rows {
        for (i, width) in widths.iter_mut().enumerate().take(row.cell_count as usize) {
            *width = (*width).max(display_width(unsafe { text(*row.cells.add(i)) }));
        }
    }
    for width in &mut widths {
        *width += 2;
    }
    let boxes = &BOXES[table.box_style.clamp(0, 5) as usize];
    if !layout_limit(
        widths
            .iter()
            .sum::<usize>()
            .saturating_add(widths.len())
            .saturating_add(1),
        rows.len().saturating_add(4),
    ) {
        return std::ptr::null();
    }
    let rule = |left: &str, line: &str, right: &str| {
        format!(
            "{left}{}{right}",
            widths
                .iter()
                .map(|width| repeat(line, *width))
                .collect::<Vec<_>>()
                .join(line)
        )
    };
    let mut result = rule(boxes[0], boxes[1], boxes[2]);
    if table.show_header != 0 {
        result.push_str(&format!(
            "\n{}{}{}\n{}",
            boxes[3],
            columns
                .iter()
                .zip(&widths)
                .map(|(column, width)| padded(unsafe { text(column.header) }, *width, true))
                .collect::<Vec<_>>()
                .join(boxes[3]),
            boxes[4],
            rule(boxes[3], boxes[1], boxes[4])
        ));
    }
    for row in rows {
        let cells = widths
            .iter()
            .enumerate()
            .map(|(i, width)| {
                let cell = if (i as i64) < row.cell_count {
                    unsafe { text(*row.cells.add(i)) }
                } else {
                    ""
                };
                format!(" {}", padded(cell, width - 1, false))
            })
            .collect::<Vec<_>>()
            .join(boxes[3]);
        result.push_str(&format!("\n{}{cells}{}", boxes[3], boxes[4]));
    }
    result.push('\n');
    result.push_str(&rule(boxes[5], boxes[6], boxes[7]));
    rendered(&result)
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Progress {
    total: i64,
    completed: i64,
    width: i64,
    description: *const c_char,
    fill_char: *const c_char,
    empty_char: *const c_char,
    show_percentage: i32,
    show_count: i32,
}
#[unsafe(no_mangle)]
pub extern "C" fn fern_progress_new(total: i64) -> *mut Progress {
    abi::owned(
        Progress {
            total: if total > 0 { total } else { 100 },
            completed: 0,
            width: 40,
            description: std::ptr::null(),
            fill_char: c"█".as_ptr(),
            empty_char: c"░".as_ptr(),
            show_percentage: 1,
            show_count: 0,
        },
        0,
    )
}
string_setter!(fern_progress_description, Progress, description);
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_progress_width(progress: *mut Progress, width: i64) -> *mut Progress {
    if let Some(progress) = unsafe { progress.as_mut() } {
        progress.width = if width > 0 { width } else { 40 };
    }
    progress
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_progress_advance(progress: *mut Progress) -> *mut Progress {
    if let Some(progress) = unsafe { progress.as_mut() }
        && progress.completed < progress.total
    {
        progress.completed += 1;
    }
    progress
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_progress_set(progress: *mut Progress, value: i64) -> *mut Progress {
    if let Some(progress) = unsafe { progress.as_mut() } {
        progress.completed = value.max(0).min(progress.total);
    }
    progress
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_progress_render(progress: *const Progress) -> *const c_char {
    let Some(progress) = (unsafe { progress.as_ref() }) else {
        return abi::string("");
    };
    let ratio = if progress.total > 0 {
        progress.completed as f64 / progress.total as f64
    } else {
        0.0
    };
    let width = progress.width.max(0) as usize;
    if !layout_limit(width, 1) {
        return std::ptr::null();
    }
    let filled = ((ratio * width as f64) as usize).min(width);
    let mut result = if progress.description.is_null() {
        String::new()
    } else {
        format!("{} ", unsafe { text(progress.description) })
    };
    result.push_str(&format!(
        "[{}{}]",
        repeat(unsafe { text(progress.fill_char) }, filled),
        repeat(unsafe { text(progress.empty_char) }, width - filled)
    ));
    if progress.show_percentage != 0 {
        result.push_str(&format!(" {:3}%", (ratio * 100.0) as i64));
    }
    if progress.show_count != 0 {
        result.push_str(&format!(" ({}/{})", progress.completed, progress.total));
    }
    rendered(&result)
}

const FRAMES: [&[&str]; 7] = [
    &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    &["-", "\\", "|", "/"],
    &["◐", "◓", "◑", "◒"],
    &["◰", "◳", "◲", "◱"],
    &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"],
    &["⠁", "⠂", "⠄", "⠂"],
    &[
        "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛",
    ],
];
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Spinner {
    style: i32,
    frame: i32,
    message: *const c_char,
}
#[unsafe(no_mangle)]
pub extern "C" fn fern_spinner_new() -> *mut Spinner {
    abi::owned(
        Spinner {
            style: 0,
            frame: 0,
            message: std::ptr::null(),
        },
        0,
    )
}
string_setter!(fern_spinner_message, Spinner, message);
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_spinner_style(
    spinner: *mut Spinner,
    style: *const c_char,
) -> *mut Spinner {
    if let Some(spinner) = unsafe { spinner.as_mut() }
        && !style.is_null()
    {
        if let Some(style) = [
            "dots", "line", "circle", "square", "arrow", "bounce", "clock",
        ]
        .iter()
        .position(|name| *name == unsafe { text(style) })
        {
            spinner.style = style as i32;
        }
        spinner.frame = 0;
    }
    spinner
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_spinner_tick(spinner: *mut Spinner) -> *mut Spinner {
    if let Some(spinner) = unsafe { spinner.as_mut() } {
        spinner.frame =
            (spinner.frame + 1) % FRAMES[spinner.style.clamp(0, 6) as usize].len() as i32;
    }
    spinner
}
#[unsafe(no_mangle)]
/// # Safety
/// Non-null string pointers must reference readable NUL-terminated storage.
/// Object and list pointers must use the declared runtime ABI, remain live for
/// this call, and allow any requested mutation without aliasing or concurrent access.
pub unsafe extern "C" fn fern_spinner_render(spinner: *const Spinner) -> *const c_char {
    let Some(spinner) = (unsafe { spinner.as_ref() }) else {
        return abi::string("");
    };
    let frame = FRAMES
        .get(spinner.style as usize)
        .and_then(|frames| frames.get(spinner.frame as usize))
        .copied()
        .unwrap_or("?");
    rendered(&if spinner.message.is_null() {
        frame.to_owned()
    } else {
        format!("{frame} {}", unsafe { text(spinner.message) })
    })
}

macro_rules! release { ($($name:ident: $kind:ty),* $(,)?) => { $(#[unsafe(no_mangle)] pub extern "C" fn $name(_: *mut $kind) {})* }; }
release!(fern_panel_free: Panel, fern_table_free: Table, fern_progress_free: Progress, fern_spinner_free: Spinner);
