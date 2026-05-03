use super::Theme;
use crate::cli::{Cli, InheritedOutputMode, OutputFormat, command_requests_robot_json};
use crate::format::{sanitize_terminal_inline, sanitize_terminal_text};
use rich_rust::prelude::*;
use rich_rust::renderables::Renderable;
use serde::Serialize;
use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};
use std::sync::OnceLock;
use toon_rust::options::KeyFoldingMode;
use toon_rust::{EncodeOptions, JsonValue, StringOrNumberOrBoolOrNull, encode_lines};

/// Central output coordinator that respects robot/json/quiet modes.
///
/// Uses lazy initialization for console and theme to ensure zero overhead
/// in JSON/Quiet modes where rich output is never used.
pub struct OutputContext {
    /// Output mode (always set eagerly - cheap)
    mode: OutputMode,
    /// Terminal width (cached, lazy)
    width: OnceLock<usize>,
    /// Rich console for human-readable output (lazy)
    console: OnceLock<Console>,
    /// Theme for consistent styling (lazy)
    theme: OnceLock<Theme>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Full rich formatting (tables, colors, panels)
    Rich,
    /// Plain text, no ANSI codes (for piping)
    Plain,
    /// JSON output only
    Json,
    /// TOON format (token-optimized object notation)
    Toon,
    /// Minimal output (quiet mode)
    Quiet,
}

const JSON_OUTPUT_BUFFER_CAPACITY: usize = 128 * 1024;

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl CountingWriter {
    const fn len(&self) -> usize {
        self.bytes
    }
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_json_array_to_writer<I, T, W>(writer: &mut W, values: I) -> serde_json::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Serialize,
    W: Write,
{
    writer.write_all(b"[").map_err(serde_json::Error::io)?;
    let mut first = true;
    for value in values {
        if first {
            first = false;
        } else {
            writer.write_all(b",").map_err(serde_json::Error::io)?;
        }
        serde_json::to_writer(&mut *writer, &value)?;
    }
    writer.write_all(b"]").map_err(serde_json::Error::io)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JsonArrayPageMeta {
    pub(crate) total: usize,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
    pub(crate) has_more: bool,
}

fn write_json_array_page_to_writer<I, T, W>(
    writer: &mut W,
    array_field: &str,
    values: I,
    meta: JsonArrayPageMeta,
) -> serde_json::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Serialize,
    W: Write,
{
    writer.write_all(b"{").map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, array_field)?;
    writer.write_all(b":").map_err(serde_json::Error::io)?;
    write_json_array_to_writer(writer, values)?;
    writer
        .write_all(b",\"total\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &meta.total)?;
    writer
        .write_all(b",\"limit\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &meta.limit)?;
    writer
        .write_all(b",\"offset\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &meta.offset)?;
    writer
        .write_all(b",\"has_more\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &meta.has_more)?;
    writer.write_all(b"}").map_err(serde_json::Error::io)
}

fn write_json_array_count_to_writer<I, T, W>(
    writer: &mut W,
    array_field: &str,
    values: I,
    count_field: &str,
    count: usize,
) -> serde_json::Result<()>
where
    I: IntoIterator<Item = T>,
    T: Serialize,
    W: Write,
{
    writer.write_all(b"{").map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, array_field)?;
    writer.write_all(b":").map_err(serde_json::Error::io)?;
    write_json_array_to_writer(writer, values)?;
    writer.write_all(b",").map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, count_field)?;
    writer.write_all(b":").map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &count)?;
    writer.write_all(b"}").map_err(serde_json::Error::io)
}

fn write_json_trailer_to_writer<W: Write>(writer: &mut W) -> serde_json::Result<()> {
    writer.write_all(b"\n").map_err(serde_json::Error::io)?;
    writer.flush().map_err(serde_json::Error::io)
}

fn write_toon_lines_to_writer<W: Write>(
    writer: &mut W,
    lines: &[String],
) -> serde_json::Result<()> {
    let mut first = true;
    for line in lines {
        if first {
            first = false;
        } else {
            writer.write_all(b"\n").map_err(serde_json::Error::io)?;
        }
        writer
            .write_all(line.as_bytes())
            .map_err(serde_json::Error::io)?;
    }
    Ok(())
}

fn toon_lines_len(lines: &[String]) -> usize {
    lines.iter().map(String::len).sum::<usize>() + lines.len().saturating_sub(1)
}

fn is_broken_pipe_serialization_error(err: &serde_json::Error) -> bool {
    err.io_error_kind() == Some(io::ErrorKind::BrokenPipe)
}

#[must_use]
fn toon_encode_options() -> EncodeOptions {
    EncodeOptions {
        indent: Some(2),
        delimiter: None,
        key_folding: Some(KeyFoldingMode::Safe),
        flatten_depth: None,
        replacer: None,
    }
}

fn sanitize_toon_value(value: &mut JsonValue) {
    match value {
        JsonValue::Primitive(StringOrNumberOrBoolOrNull::String(value)) => {
            if let Cow::Owned(safe_value) = sanitize_toon_string(value) {
                *value = safe_value;
            }
        }
        JsonValue::Primitive(
            StringOrNumberOrBoolOrNull::Null
            | StringOrNumberOrBoolOrNull::Bool(_)
            | StringOrNumberOrBoolOrNull::Number(_),
        ) => {}
        JsonValue::Array(values) => {
            for value in values {
                sanitize_toon_value(value);
            }
        }
        JsonValue::Object(values) => {
            for (key, value) in values {
                if let Cow::Owned(safe_key) = sanitize_toon_string(key) {
                    *key = safe_key;
                }
                sanitize_toon_value(value);
            }
        }
    }
}

fn sanitize_toon_string(value: &str) -> Cow<'_, str> {
    if ascii_toon_string_is_clean(value).unwrap_or_else(|| {
        value
            .chars()
            .all(|ch| matches!(ch, '\n' | '\t') || !ch.is_control())
    }) {
        return Cow::Borrowed(value);
    }

    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\n' | '\t') || !ch.is_control() {
            escaped.push(ch);
            continue;
        }

        for escaped_char in ch.escape_default() {
            escaped.push(escaped_char);
        }
    }

    Cow::Owned(escaped)
}

fn ascii_toon_string_is_clean(value: &str) -> Option<bool> {
    let mut saw_non_ascii = false;
    for byte in value.bytes() {
        match byte {
            b'\n' | b'\t' => {}
            0x00..=0x1f | 0x7f => return Some(false),
            0x80..=0xff => saw_non_ascii = true,
            _ => {}
        }
    }
    (!saw_non_ascii).then_some(true)
}

impl OutputContext {
    /// Detect output mode from environment and terminal state without CLI args.
    #[must_use]
    pub fn detect() -> Self {
        if let Some(format) = OutputFormat::from_env() {
            return Self::from_output_format(format, false, false);
        }
        Self::from_flags(false, false, false)
    }

    /// Create a context with an explicit mode.
    #[must_use]
    pub fn with_mode(mode: OutputMode) -> Self {
        Self {
            mode,
            width: OnceLock::new(),
            console: OnceLock::new(),
            theme: OnceLock::new(),
        }
    }

    /// Create from CLI global args.
    ///
    /// Only mode is set eagerly; console/theme/width are lazy-initialized
    /// on first access to ensure zero overhead in JSON/Quiet modes.
    #[must_use]
    pub fn from_args(args: &Cli) -> Self {
        Self {
            mode: Self::detect_mode(args),
            width: OnceLock::new(),
            console: OnceLock::new(),
            theme: OnceLock::new(),
        }
    }

    /// Create from CLI-style flags.
    ///
    /// Only mode is set eagerly; console/theme/width are lazy-initialized
    /// on first access to ensure zero overhead in JSON/Quiet modes.
    #[must_use]
    pub fn from_flags(json: bool, quiet: bool, no_color: bool) -> Self {
        let mode = if json {
            OutputMode::Json
        } else if quiet {
            OutputMode::Quiet
        } else if no_color || std::env::var("NO_COLOR").is_ok() || !std::io::stdout().is_terminal()
        {
            OutputMode::Plain
        } else {
            OutputMode::Rich
        };

        Self {
            mode,
            width: OnceLock::new(),
            console: OnceLock::new(),
            theme: OnceLock::new(),
        }
    }

    /// Create from an explicit output format.
    #[must_use]
    pub fn from_output_format(format: OutputFormat, quiet: bool, no_color: bool) -> Self {
        let mode = match format {
            OutputFormat::Json => OutputMode::Json,
            OutputFormat::Toon => OutputMode::Toon,
            OutputFormat::Text | OutputFormat::Csv => {
                if quiet {
                    OutputMode::Quiet
                } else if no_color
                    || std::env::var("NO_COLOR").is_ok()
                    || !std::io::stdout().is_terminal()
                {
                    OutputMode::Plain
                } else {
                    OutputMode::Rich
                }
            }
        };

        Self {
            mode,
            width: OnceLock::new(),
            console: OnceLock::new(),
            theme: OnceLock::new(),
        }
    }

    fn detect_mode(args: &Cli) -> OutputMode {
        Self::detect_mode_with_env(args, OutputFormat::from_env())
    }

    fn detect_mode_with_env(args: &Cli, env_output_format: Option<OutputFormat>) -> OutputMode {
        if args.json || command_requests_robot_json(&args.command) {
            return OutputMode::Json;
        }
        if args.quiet {
            return OutputMode::Quiet;
        }
        if let Some(format) = env_output_format {
            match format {
                OutputFormat::Json => return OutputMode::Json,
                OutputFormat::Toon => return OutputMode::Toon,
                OutputFormat::Text | OutputFormat::Csv => {}
            }
        }
        if args.no_color || std::env::var("NO_COLOR").is_ok() {
            return OutputMode::Plain;
        }
        if !std::io::stdout().is_terminal() {
            return OutputMode::Plain;
        }
        OutputMode::Rich
    }

    /// Lazily create console based on mode.
    fn console(&self) -> &Console {
        self.console.get_or_init(|| match self.mode {
            OutputMode::Rich => Console::new(),
            OutputMode::Plain | OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {
                Console::builder().no_color().force_terminal(false).build()
            }
        })
    }

    // ─────────────────────────────────────────────────────────────
    // Mode Checks (no lazy initialization needed - mode is always set)
    // ─────────────────────────────────────────────────────────────

    pub fn mode(&self) -> OutputMode {
        self.mode
    }
    pub fn is_rich(&self) -> bool {
        self.mode == OutputMode::Rich
    }
    pub fn is_json(&self) -> bool {
        self.mode == OutputMode::Json
    }
    pub fn is_toon(&self) -> bool {
        self.mode == OutputMode::Toon
    }
    pub fn is_quiet(&self) -> bool {
        self.mode == OutputMode::Quiet
    }
    pub fn is_plain(&self) -> bool {
        self.mode == OutputMode::Plain
    }

    pub const fn inherited_output_mode(&self) -> InheritedOutputMode {
        match self.mode {
            OutputMode::Json => InheritedOutputMode::Json,
            OutputMode::Toon => InheritedOutputMode::Toon,
            OutputMode::Quiet => InheritedOutputMode::Quiet,
            OutputMode::Rich | OutputMode::Plain => InheritedOutputMode::None,
        }
    }

    /// Get terminal width (lazy-initialized).
    pub fn width(&self) -> usize {
        *self.width.get_or_init(|| self.console().width())
    }

    /// Get theme (lazy-initialized).
    ///
    /// In JSON/Quiet modes, this is never called, so theme is never created.
    pub fn theme(&self) -> &Theme {
        self.theme.get_or_init(Theme::default)
    }

    // ─────────────────────────────────────────────────────────────
    // Output Methods
    // ─────────────────────────────────────────────────────────────

    pub fn print(&self, content: &str) {
        let content = sanitize_terminal_text(content);
        match self.mode {
            OutputMode::Rich | OutputMode::Plain => {
                self.console()
                    .print_renderable(&Text::new(content.into_owned()));
            }
            OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {} // No console access - zero overhead
        }
    }

    pub fn print_line(&self, content: &str) {
        let content = sanitize_terminal_text(content);
        match self.mode {
            OutputMode::Rich => {
                let mut text = Text::new(content.into_owned());
                text.append("\n");
                self.console().print_renderable(&text);
            }
            OutputMode::Plain => println!("{content}"),
            OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {}
        }
    }

    pub fn render<R: Renderable>(&self, renderable: &R) {
        if self.is_rich() {
            self.console().print_renderable(renderable);
        }
    }

    fn report_serialization_error(&self, format: &str, err: &serde_json::Error) {
        if !self.is_quiet() && !is_broken_pipe_serialization_error(err) {
            eprintln!("Error: failed to serialize {format} output: {err}");
        }
    }

    fn json_value<T: serde::Serialize>(
        &self,
        value: &T,
        format: &str,
    ) -> Option<serde_json::Value> {
        match serde_json::to_value(value) {
            Ok(json_value) => Some(json_value),
            Err(err) => {
                self.report_serialization_error(format, &err);
                None
            }
        }
    }

    pub fn json<T: serde::Serialize>(&self, value: &T) {
        if self.is_json() {
            // Stream to stdout to avoid allocating large JSON strings.
            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) = serde_json::to_writer(&mut out, value) {
                self.report_serialization_error("JSON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("JSON", &err);
            }
        }
    }

    pub fn json_array<I, T>(&self, values: I)
    where
        I: IntoIterator<Item = T>,
        T: serde::Serialize,
    {
        if self.is_json() {
            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) = write_json_array_to_writer(&mut out, values) {
                self.report_serialization_error("JSON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("JSON", &err);
            }
        }
    }

    pub(crate) fn json_array_page<I, T>(
        &self,
        array_field: &str,
        values: I,
        meta: JsonArrayPageMeta,
    ) where
        I: IntoIterator<Item = T>,
        T: serde::Serialize,
    {
        if self.is_json() {
            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) = write_json_array_page_to_writer(&mut out, array_field, values, meta) {
                self.report_serialization_error("JSON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("JSON", &err);
            }
        }
    }

    pub(crate) fn json_array_count<I, T>(
        &self,
        array_field: &str,
        values: I,
        count_field: &str,
        count: usize,
    ) where
        I: IntoIterator<Item = T>,
        T: serde::Serialize,
    {
        if self.is_json() {
            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) =
                write_json_array_count_to_writer(&mut out, array_field, values, count_field, count)
            {
                self.report_serialization_error("JSON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("JSON", &err);
            }
        }
    }

    pub fn json_pretty<T: serde::Serialize>(&self, value: &T) {
        if self.is_rich() {
            let Some(json_value) = self.json_value(value, "JSON") else {
                return;
            };
            let json = rich_rust::renderables::Json::new(json_value);
            self.console().print_renderable(&json);
        } else if self.is_json() {
            self.json(value);
        }
    }

    /// Output value as TOON format (token-optimized object notation).
    pub fn toon<T: serde::Serialize>(&self, value: &T) {
        if self.is_toon() {
            let Some(json_value) = self.json_value(value, "TOON") else {
                return;
            };
            let mut toon_value: JsonValue = json_value.into();
            sanitize_toon_value(&mut toon_value);
            let options = Some(toon_encode_options());
            let toon_lines = encode_lines(toon_value, options);
            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) = write_toon_lines_to_writer(&mut out, &toon_lines) {
                self.report_serialization_error("TOON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("TOON", &err);
            }
        }
    }

    const fn should_emit_toon_stats(show_stats: bool, env_enabled: bool) -> bool {
        show_stats || env_enabled
    }

    fn pretty_json_len(value: &serde_json::Value) -> Option<usize> {
        let mut writer = CountingWriter::default();
        let mut serializer = serde_json::Serializer::pretty(&mut writer);
        value.serialize(&mut serializer).ok()?;
        Some(writer.len())
    }

    /// Output value as TOON format with optional stats on stderr.
    pub fn toon_with_stats<T: serde::Serialize>(&self, value: &T, show_stats: bool) {
        if self.is_toon() {
            let Some(json_value) = self.json_value(value, "TOON") else {
                return;
            };
            let mut toon_value: JsonValue = json_value.into();
            sanitize_toon_value(&mut toon_value);
            let emit_stats =
                Self::should_emit_toon_stats(show_stats, std::env::var("TOON_STATS").is_ok());
            let json_chars = if emit_stats {
                let sanitized_json_value: serde_json::Value = toon_value.clone().into();
                Self::pretty_json_len(&sanitized_json_value)
            } else {
                None
            };
            let options = Some(toon_encode_options());
            let toon_lines = encode_lines(toon_value, options);
            let toon_chars = toon_lines_len(&toon_lines);

            if let Some(json_chars) = json_chars {
                let savings = if json_chars > 0 {
                    let diff = json_chars.saturating_sub(toon_chars);
                    diff * 100 / json_chars
                } else {
                    0
                };
                eprintln!(
                    "[stats] JSON: {} chars, TOON: {} chars ({}% savings)",
                    json_chars, toon_chars, savings
                );
            }

            let stdout = io::stdout();
            let mut out = io::BufWriter::with_capacity(JSON_OUTPUT_BUFFER_CAPACITY, stdout.lock());
            if let Err(err) = write_toon_lines_to_writer(&mut out, &toon_lines) {
                self.report_serialization_error("TOON", &err);
                return;
            }
            if let Err(err) = write_json_trailer_to_writer(&mut out) {
                self.report_serialization_error("TOON", &err);
            }
        }
    }

    // ─────────────────────────────────────────────────────────────
    // Semantic Output Methods
    // ─────────────────────────────────────────────────────────────

    pub fn success(&self, message: &str) {
        let message = sanitize_terminal_inline(message);
        match self.mode {
            OutputMode::Rich => {
                let mut text = Text::new("");
                text.append_styled("✓", self.theme().success.clone().bold());
                text.append(" ");
                text.append(message.as_ref());
                text.append("\n");
                self.console().print_renderable(&text);
            }
            OutputMode::Plain => println!("✓ {}", message),
            OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {} //
        }
    }

    pub fn error(&self, message: &str) {
        let message = sanitize_terminal_text(message);
        match self.mode {
            OutputMode::Rich => {
                let panel = Panel::from_text(message.as_ref())
                    .title(Text::new("Error"))
                    .border_style(self.theme().error.clone());
                self.console().print_renderable(&panel);
            }
            OutputMode::Plain | OutputMode::Quiet => eprintln!("Error: {}", message),
            OutputMode::Json | OutputMode::Toon => {} //
        }
    }

    pub fn warning(&self, message: &str) {
        let message = sanitize_terminal_inline(message);
        match self.mode {
            OutputMode::Rich => {
                let mut text = Text::new("");
                text.append_styled("⚠", self.theme().warning.clone().bold());
                text.append(" ");
                text.append_styled(message.as_ref(), self.theme().warning.clone());
                text.append("\n");
                self.console().print_renderable(&text);
            }
            OutputMode::Plain => eprintln!("Warning: {}", message),
            OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {} //
        }
    }

    pub fn info(&self, message: &str) {
        let message = sanitize_terminal_inline(message);
        match self.mode {
            OutputMode::Rich => {
                let mut text = Text::new("");
                text.append_styled("ℹ", self.theme().info.clone());
                text.append(" ");
                text.append(message.as_ref());
                text.append("\n");
                self.console().print_renderable(&text);
            }
            OutputMode::Plain => println!("{}", message),
            OutputMode::Quiet | OutputMode::Json | OutputMode::Toon => {} //
        }
    }

    pub fn section(&self, title: &str) {
        let title = sanitize_terminal_inline(title);
        if self.is_rich() {
            let rule =
                Rule::with_title(Text::new(title.into_owned())).style(self.theme().section.clone());
            self.console().print_renderable(&rule);
        } else if self.is_plain() {
            println!("\n─── {} ───\n", title);
        }
    }

    pub fn newline(&self) {
        if !self.is_quiet() && !self.is_json() && !self.is_toon() {
            println!();
        }
    }

    pub fn error_panel(&self, title: &str, description: &str, suggestions: &[&str]) {
        let title = sanitize_terminal_inline(title);
        let description = sanitize_terminal_text(description);
        match self.mode {
            OutputMode::Rich => {
                let mut text = Text::from(description.as_ref());
                text.append("\n\nSuggestions:\n");
                for suggestion in suggestions {
                    let suggestion = sanitize_terminal_inline(suggestion);
                    text.append("• ");
                    text.append(suggestion.as_ref());
                    text.append("\n");
                }

                let panel = Panel::from_rich_text(&text, self.width())
                    .title(Text::new(title.as_ref()))
                    .border_style(self.theme().error.clone());
                self.console().print_renderable(&panel);
            }
            OutputMode::Plain => {
                eprintln!("Error: {} - {}", title, description);
                for suggestion in suggestions {
                    eprintln!("  Suggestion: {}", sanitize_terminal_inline(suggestion));
                }
            }
            OutputMode::Quiet => eprintln!("Error: {}", description),
            OutputMode::Json | OutputMode::Toon => {} //
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde::Serialize;
    use serde::ser::Error as _;
    use serde_json::json;

    struct FailingSerialize;

    impl Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(S::Error::custom("boom"))
        }
    }

    #[test]
    fn detect_mode_uses_env_json_default_when_no_explicit_format_requested() {
        let cli = Cli::parse_from(["br", "count"]);
        assert_eq!(
            OutputContext::detect_mode_with_env(&cli, Some(OutputFormat::Json)),
            OutputMode::Json
        );
    }

    #[test]
    fn detect_mode_uses_env_toon_default_when_no_explicit_format_requested() {
        let cli = Cli::parse_from(["br", "count"]);
        assert_eq!(
            OutputContext::detect_mode_with_env(&cli, Some(OutputFormat::Toon)),
            OutputMode::Toon
        );
    }

    #[test]
    fn detect_mode_quiet_overrides_env_machine_format() {
        let cli = Cli::parse_from(["br", "--quiet", "count"]);
        assert_eq!(
            OutputContext::detect_mode_with_env(&cli, Some(OutputFormat::Json)),
            OutputMode::Quiet
        );
    }

    #[test]
    fn detect_mode_explicit_json_overrides_env_toon_default() {
        let cli = Cli::parse_from(["br", "--json", "count"]);
        assert_eq!(
            OutputContext::detect_mode_with_env(&cli, Some(OutputFormat::Toon)),
            OutputMode::Json
        );
    }

    #[test]
    fn detect_mode_uses_robot_flag_for_sync() {
        let cli = Cli::parse_from(["br", "sync", "--robot"]);
        assert_eq!(
            OutputContext::detect_mode_with_env(&cli, Some(OutputFormat::Text)),
            OutputMode::Json
        );
    }

    #[test]
    fn detect_mode_global_flag_matrix_has_unambiguous_precedence() {
        for quiet in [false, true] {
            for json in [false, true] {
                for robot in [false, true] {
                    for no_color in [false, true] {
                        let mut argv = vec!["br"];
                        if quiet {
                            argv.push("--quiet");
                        }
                        if json {
                            argv.push("--json");
                        }
                        if no_color {
                            argv.push("--no-color");
                        }
                        argv.extend(["sync", "--status"]);
                        if robot {
                            argv.push("--robot");
                        }

                        let cli = Cli::parse_from(argv);
                        let mode = OutputContext::detect_mode_with_env(&cli, None);

                        if json || robot {
                            assert_eq!(
                                mode,
                                OutputMode::Json,
                                "json/robot must override quiet/no-color: quiet={quiet}, json={json}, robot={robot}, no_color={no_color}"
                            );
                        } else if quiet {
                            assert_eq!(
                                mode,
                                OutputMode::Quiet,
                                "quiet must override no-color: quiet={quiet}, json={json}, robot={robot}, no_color={no_color}"
                            );
                        } else if no_color {
                            assert_eq!(
                                mode,
                                OutputMode::Plain,
                                "no-color must force plain output: quiet={quiet}, json={json}, robot={robot}, no_color={no_color}"
                            );
                        } else {
                            assert!(
                                matches!(mode, OutputMode::Rich | OutputMode::Plain),
                                "no explicit output controls should be TTY-dependent, got {mode:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn detect_mode_short_quiet_alias_matches_long_quiet() {
        let short = Cli::parse_from(["br", "-q", "sync", "--status"]);
        let long = Cli::parse_from(["br", "--quiet", "sync", "--status"]);

        assert_eq!(
            OutputContext::detect_mode_with_env(&short, None),
            OutputContext::detect_mode_with_env(&long, None)
        );
    }

    #[test]
    fn should_emit_toon_stats_when_flag_is_set() {
        assert!(OutputContext::should_emit_toon_stats(true, false));
    }

    #[test]
    fn should_emit_toon_stats_when_env_is_set() {
        assert!(OutputContext::should_emit_toon_stats(false, true));
    }

    #[test]
    fn should_not_emit_toon_stats_when_flag_and_env_are_absent() {
        assert!(!OutputContext::should_emit_toon_stats(false, false));
    }

    #[test]
    fn pretty_json_len_matches_pretty_serializer_output() {
        let value = json!({
            "title": "CLI issue",
            "labels": ["cli", "perf"],
            "nested": { "priority": 2, "status": "open" }
        });

        assert_eq!(
            OutputContext::pretty_json_len(&value),
            Some(
                serde_json::to_string_pretty(&value)
                    .expect("JSON serialization failed")
                    .len()
            )
        );
    }

    #[test]
    fn write_json_array_to_writer_matches_materialized_vec_output() {
        #[derive(Serialize)]
        struct Row {
            id: &'static str,
            priority: u8,
        }

        let rows = vec![
            Row {
                id: "beads_rust-alpha",
                priority: 0,
            },
            Row {
                id: "beads_rust-beta",
                priority: 1,
            },
        ];
        let mut streamed = Vec::new();

        write_json_array_to_writer(&mut streamed, rows.iter())
            .expect("streaming JSON array serialization failed");

        assert_eq!(
            streamed,
            serde_json::to_vec(&rows).expect("materialized JSON serialization failed")
        );
    }

    #[test]
    fn write_json_array_to_writer_emits_empty_array() {
        let mut streamed = Vec::new();

        write_json_array_to_writer(&mut streamed, std::iter::empty::<serde_json::Value>())
            .expect("streaming empty JSON array serialization failed");

        assert_eq!(streamed, b"[]");
    }

    #[test]
    fn write_json_array_page_to_writer_matches_materialized_page_output() {
        #[derive(Serialize)]
        struct Row {
            id: &'static str,
            priority: u8,
        }

        #[derive(Serialize)]
        struct Page<'a> {
            issues: &'a [Row],
            total: usize,
            limit: usize,
            offset: usize,
            has_more: bool,
        }

        let rows = vec![
            Row {
                id: "beads_rust-alpha",
                priority: 0,
            },
            Row {
                id: "beads_rust-beta",
                priority: 1,
            },
        ];
        let meta = JsonArrayPageMeta {
            total: 5,
            limit: 2,
            offset: 1,
            has_more: true,
        };
        let mut streamed = Vec::new();

        write_json_array_page_to_writer(&mut streamed, "issues", rows.iter(), meta)
            .expect("streaming JSON page serialization failed");

        let materialized = Page {
            issues: &rows,
            total: meta.total,
            limit: meta.limit,
            offset: meta.offset,
            has_more: meta.has_more,
        };
        assert_eq!(
            streamed,
            serde_json::to_vec(&materialized).expect("materialized JSON page serialization failed")
        );
    }

    #[test]
    fn write_json_array_count_to_writer_matches_materialized_struct_output() {
        #[derive(Serialize)]
        struct QueryList<'a> {
            queries: &'a [serde_json::Value],
            count: usize,
        }

        let rows = vec![json!({"name": "triage"}), json!({"name": "release"})];
        let materialized = QueryList {
            queries: &rows,
            count: rows.len(),
        };
        let mut streamed = Vec::new();

        write_json_array_count_to_writer(
            &mut streamed,
            "queries",
            rows.iter(),
            "count",
            rows.len(),
        )
        .expect("streaming JSON query list serialization failed");

        assert_eq!(
            streamed,
            serde_json::to_vec(&materialized)
                .expect("materialized JSON query list serialization failed")
        );
    }

    #[test]
    fn write_toon_lines_to_writer_matches_materialized_encode_output() {
        let value = json!({
            "issues": [
                { "id": "beads_rust-alpha", "priority": 0 },
                { "id": "beads_rust-beta", "priority": 1 }
            ],
            "count": 2
        });
        let mut toon_value = JsonValue::from(value);
        sanitize_toon_value(&mut toon_value);
        let options = Some(toon_encode_options());
        let materialized = toon_rust::encode(toon_value.clone(), options.clone()).into_bytes();
        let lines = encode_lines(toon_value, options);
        let mut streamed = Vec::new();

        write_toon_lines_to_writer(&mut streamed, &lines)
            .expect("streaming TOON line output failed");

        assert_eq!(streamed, materialized);
        assert_eq!(toon_lines_len(&lines), streamed.len());
    }

    struct WriteZero;

    impl Write for WriteZero {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Ok(0)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct BrokenPipeOnFlush {
        bytes: Vec<u8>,
    }

    impl Write for BrokenPipeOnFlush {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
    }

    #[test]
    fn write_json_array_to_writer_propagates_partial_writer_failure() {
        let err = write_json_array_to_writer(&mut WriteZero, [json!({"id": "bd-a"})].iter())
            .expect_err("partial writer should fail");

        assert_eq!(err.io_error_kind(), Some(io::ErrorKind::WriteZero));
        assert!(!is_broken_pipe_serialization_error(&err));
    }

    #[test]
    fn write_toon_lines_to_writer_propagates_partial_writer_failure() {
        let lines = vec!["items[1]:".to_string(), "  - beads_rust-alpha".to_string()];
        let err =
            write_toon_lines_to_writer(&mut WriteZero, &lines).expect_err("partial writer failed");

        assert_eq!(err.io_error_kind(), Some(io::ErrorKind::WriteZero));
        assert!(!is_broken_pipe_serialization_error(&err));
    }

    #[test]
    fn write_json_trailer_flushes_and_classifies_broken_pipe() {
        let mut writer = BrokenPipeOnFlush { bytes: Vec::new() };
        let err =
            write_json_trailer_to_writer(&mut writer).expect_err("flush should report broken pipe");

        assert_eq!(writer.bytes, b"\n");
        assert_eq!(err.io_error_kind(), Some(io::ErrorKind::BrokenPipe));
        assert!(is_broken_pipe_serialization_error(&err));
    }

    #[test]
    fn sanitize_toon_string_keeps_newline_and_tab_but_escapes_carriage_return() {
        assert_eq!(sanitize_toon_string("line\n\t\rnext"), "line\n\t\\rnext");
    }

    #[test]
    fn sanitize_toon_value_escapes_controls_the_encoder_would_emit_raw() {
        let value = json!({
            "plain": "ok",
            "bad\u{1b}key": "title\u{1b}[2J\u{7}\u{9b}\u{8}\n\t\rend",
            "nested": [
                { "body": "bell\u{7}" }
            ]
        });

        let mut toon_value = JsonValue::from(value);
        sanitize_toon_value(&mut toon_value);
        let toon_output = toon_rust::encode(toon_value, Some(toon_encode_options()));

        for forbidden in ['\u{1b}', '\u{7}', '\u{8}', '\u{9b}', '\r'] {
            assert!(
                !toon_output.contains(forbidden),
                "TOON output contained raw control {forbidden:?}: {toon_output:?}"
            );
        }

        assert!(toon_output.contains("\\u{1b}[2J"));
        assert!(toon_output.contains("\\u{7}"));
        assert!(toon_output.contains("\\u{8}"));
        assert!(toon_output.contains("\\u{9b}"));
        assert!(toon_output.contains("\\n"));
        assert!(toon_output.contains("\\t"));
        assert!(toon_output.contains("\\r"));
    }

    #[test]
    fn sanitize_toon_value_keeps_entries_when_sanitized_keys_collide() {
        let mut toon_value = JsonValue::Object(vec![
            (
                "bad\u{1b}".to_string(),
                JsonValue::Primitive(StringOrNumberOrBoolOrNull::String("first".to_string())),
            ),
            (
                "bad\\u{1b}".to_string(),
                JsonValue::Primitive(StringOrNumberOrBoolOrNull::String("second".to_string())),
            ),
        ]);

        sanitize_toon_value(&mut toon_value);

        let entries = match toon_value {
            JsonValue::Object(entries) => entries,
            JsonValue::Primitive(_) | JsonValue::Array(_) => Vec::new(),
        };
        let keys = entries
            .into_iter()
            .map(|(key, _value)| key)
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            vec!["bad\\u{1b}".to_string(), "bad\\u{1b}".to_string()]
        );
    }

    #[test]
    fn json_value_returns_none_on_serialize_error() {
        let ctx = OutputContext::from_output_format(OutputFormat::Json, false, true);
        assert!(ctx.json_value(&FailingSerialize, "JSON").is_none());
    }

    fn rich_test_context() -> OutputContext {
        OutputContext {
            mode: OutputMode::Rich,
            width: std::sync::OnceLock::new(),
            console: std::sync::OnceLock::new(),
            theme: std::sync::OnceLock::new(),
        }
    }

    #[test]
    fn rich_status_helpers_emit_trailing_newlines() {
        let ctx = rich_test_context();
        ctx.console().begin_capture();

        ctx.success("created");
        ctx.info("details");
        ctx.warning("careful");

        let rendered: String = ctx
            .console()
            .end_capture()
            .into_iter()
            .map(|segment| segment.text.into_owned())
            .collect();

        assert!(rendered.contains("created\n"));
        assert!(rendered.contains("details\n"));
        assert!(rendered.contains("careful\n"));
    }
}
