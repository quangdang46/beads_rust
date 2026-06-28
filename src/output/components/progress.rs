use crate::format::sanitize_terminal_inline;
use crate::output::Theme;
use rich_rust::prelude::*;
use std::io::{self, Write};

/// Progress tracker for long operations (sync, import, export).
pub struct ProgressTracker {
    total: usize,
    current: usize,
    description: String,
    bar: ProgressBar,
    success_style: Style,
}

impl ProgressTracker {
    pub fn new(theme: &Theme, total: usize, description: impl Into<String>) -> Self {
        let bar = ProgressBar::with_total(total as u64)
            .width(40)
            .bar_style(BarStyle::Block)
            .completed_style(theme.accent.clone())
            .remaining_style(theme.dimmed.clone());

        Self {
            total,
            current: 0,
            description: description.into(),
            bar,
            success_style: theme.success.clone(),
        }
    }

    pub fn tick(&mut self) {
        self.current = self.current.saturating_add(1).min(self.total);
        self.bar.set_progress(self.progress_ratio());
    }

    pub fn set(&mut self, current: usize) {
        self.current = current.min(self.total);
        self.bar.set_progress(self.progress_ratio());
    }

    fn progress_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64).min(1.0)
        }
    }

    pub fn render(&self, console: &Console) {
        // Clear line and render progress
        print!("\r");
        console.print_renderable(&self.render_prefix());
        console.print_renderable(&self.bar);
        print!(" {}/{}", self.current, self.total);
        io::stdout().flush().ok();
    }

    pub fn finish(&self, console: &Console) {
        println!();
        console.print_renderable(&self.finish_message());
    }

    fn render_prefix(&self) -> Text {
        let mut prefix = Text::new("");
        prefix.append_styled(
            sanitize_terminal_inline(&self.description).as_ref(),
            Style::new().bold(),
        );
        prefix.append(": ");
        prefix
    }

    fn finish_message(&self) -> Text {
        let mut message = Text::new("");
        message.append_styled("✓", self.success_style.clone());
        message.append(" ");
        message.append(sanitize_terminal_inline(&self.description).as_ref());
        message.append(&format!(" complete ({} items)", self.total));
        message
    }

    #[cfg(test)]
    fn display_counts(&self) -> (usize, usize) {
        (self.current, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_no_control_chars(text: &str) {
        assert!(
            !text.chars().any(char::is_control),
            "rendered text contains a terminal control character: {text:?}"
        );
    }

    fn span_text(text: &Text, span: &rich_rust::Span) -> String {
        text.plain()
            .chars()
            .skip(span.start)
            .take(span.end.saturating_sub(span.start))
            .collect()
    }

    #[test]
    fn progress_prefix_sanitizes_description_without_markup_parsing() {
        let theme = Theme::default();
        let tracker = ProgressTracker::new(&theme, 3, "[red]sync[/]\x1b[2J\rreset");

        let prefix = tracker.render_prefix();
        let rendered = prefix.plain();

        assert_no_control_chars(rendered);
        assert_eq!(rendered, "[red]sync[/]\\u{1b}[2J\\rreset: ");
        assert!(!rendered.contains("[bold]"));
        assert!(
            prefix
                .spans()
                .iter()
                .any(|span| span_text(&prefix, span).contains("[red]sync[/]"))
        );
    }

    #[test]
    fn progress_finish_message_sanitizes_description_without_markup_parsing() {
        let theme = Theme::default();
        let tracker = ProgressTracker::new(&theme, 2, "import[/]\x07done");

        let message = tracker.finish_message();
        let rendered = message.plain();

        assert_no_control_chars(rendered);
        assert_eq!(rendered, "✓ import[/]\\u{7}done complete (2 items)");
        assert!(!rendered.contains("[bold green]"));
        assert!(
            message
                .spans()
                .iter()
                .any(|span| span_text(&message, span) == "✓")
        );
    }

    #[test]
    fn progress_counter_never_displays_past_total() {
        let theme = Theme::default();
        let mut tracker = ProgressTracker::new(&theme, 2, "export");

        tracker.tick();
        tracker.tick();
        tracker.tick();
        assert_eq!(tracker.display_counts(), (2, 2));

        tracker.set(usize::MAX);
        assert_eq!(tracker.display_counts(), (2, 2));
    }

    #[test]
    fn zero_total_progress_remains_at_zero_when_ticked() {
        let theme = Theme::default();
        let mut tracker = ProgressTracker::new(&theme, 0, "scan");

        tracker.tick();
        tracker.set(10);

        assert_eq!(tracker.display_counts(), (0, 0));
        assert!(tracker.progress_ratio().abs() < f64::EPSILON);
    }
}
