//! Release notes: GitHub's generated Markdown, shown readably without a Markdown engine.

use egui::{RichText, Ui};

use crate::theme::Tokens;

/// One line of notes, classified.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Heading(&'a str),
    /// A bullet: its text, and the pull request link GitHub appends (`… in https://…/pull/12`).
    Bullet(&'a str, Option<&'a str>),
    Text(&'a str),
    Blank,
}

pub(crate) fn classify(line: &str) -> Line<'_> {
    let l = line.trim();
    if l.is_empty() {
        return Line::Blank;
    }
    if let Some(h) = l.strip_prefix('#') {
        return Line::Heading(h.trim_start_matches('#').trim());
    }
    if let Some(b) = l.strip_prefix("* ").or_else(|| l.strip_prefix("- ")) {
        if let Some((text, url)) = b.rsplit_once(" in https://")
            && !url.contains(' ')
        {
            let start = b.len() - url.len() - "https://".len();
            return Line::Bullet(text, b.get(start..));
        }
        return Line::Bullet(b, None);
    }
    Line::Text(l)
}

/// The notes as the launcher shows them. Nodalia's release notes carry a Spanish and an English
/// section, a download table and the checksum footnotes; the reader gets the section in their own
/// language (English for a language the notes don't have) and none of the rest. Notes with no such
/// sections (GitHub's generated ones, or English alone) keep their lines, less anything from a
/// downloads section on. `code` is the UI language's code (`es`, `fr`, …).
pub(crate) fn normalise(notes: &str, code: &str) -> String {
    let spanish = code == "es" || code.starts_with("es-");
    let mut preamble: Vec<&str> = Vec::new();
    let mut sections: Vec<(Section, Vec<&str>)> = Vec::new();
    for line in notes.lines() {
        if let Some(title) = line.trim().strip_prefix("## ") {
            let kind = section_of(title);
            // A section that isn't a language or the downloads keeps its heading.
            sections.push((kind, if kind == Section::Other { vec![line] } else { Vec::new() }));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push(line);
        } else {
            preamble.push(line);
        }
    }
    let downloads_at = sections.iter().position(|(kind, _)| *kind == Section::Downloads);
    if let Some(at) = downloads_at {
        sections.truncate(at);
    }
    let has_languages = sections.iter().any(|(kind, _)| matches!(kind, Section::Spanish | Section::English));
    let mut out: Vec<String> = Vec::new();
    out.extend(pick_language(&preamble, spanish));
    if has_languages {
        let wanted = if spanish { Section::Spanish } else { Section::English };
        let chosen = sections
            .iter()
            .find(|(kind, _)| *kind == wanted)
            .or_else(|| sections.iter().find(|(kind, _)| matches!(kind, Section::Spanish | Section::English)));
        if let Some((_, body)) = chosen {
            out.push(String::new());
            out.extend(body.iter().map(|l| (*l).to_string()));
        }
    } else {
        // Plain sections keep their headings (GitHub's "What's Changed").
        for (_, body) in &sections {
            out.extend(body.iter().map(|l| (*l).to_string()));
        }
    }
    // A collapsed run of blank lines, and none at either end.
    let mut text = String::new();
    let mut blank = true;
    for line in out {
        let empty = line.trim().is_empty();
        if empty && blank {
            continue;
        }
        blank = empty;
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text.trim_end().to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Spanish,
    English,
    Downloads,
    Other,
}

fn section_of(title: &str) -> Section {
    let t = title.trim().to_lowercase();
    if t.contains("descarga") || t.contains("download") {
        Section::Downloads
    } else if t == "español" || t == "spanish" {
        Section::Spanish
    } else if t == "english" {
        Section::English
    } else {
        Section::Other
    }
}

/// The lines before the first section: a one-line summary in Spanish and its English twin in italics,
/// and sometimes a GitHub alert (`> [!IMPORTANT]`) with the same two languages. Keeps the reader's.
fn pick_language(lines: &[&str], spanish: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    let flush_quote = |quote: &mut Vec<String>, out: &mut Vec<String>| {
        let body: Vec<String> = quote.drain(..).filter(|l| !l.is_empty()).collect();
        // The alert's two lines are Spanish then English; one line is for everyone.
        let pick = match body.as_slice() {
            [only] => Some(only.clone()),
            [es, en, ..] => Some(if spanish { es.clone() } else { en.clone() }),
            [] => None,
        };
        out.extend(pick);
    };
    let mut summary: Vec<&str> = Vec::new();
    for line in lines {
        let l = line.trim();
        if let Some(q) = l.strip_prefix('>') {
            let q = q.trim();
            if !q.starts_with("[!") {
                quote.push(q.to_string());
            }
            continue;
        }
        flush_quote(&mut quote, &mut out);
        if !l.is_empty() {
            summary.push(l);
        } else if !summary.is_empty() {
            out.extend(pick_summary(&summary, spanish));
            summary.clear();
            out.push(String::new());
        }
    }
    flush_quote(&mut quote, &mut out);
    out.extend(pick_summary(&summary, spanish));
    out
}

/// One paragraph of summary lines: the italic ones are the English, the others the Spanish.
fn pick_summary(lines: &[&str], spanish: bool) -> Vec<String> {
    let italic = |l: &str| l.len() > 2 && l.starts_with('*') && l.ends_with('*') && !l.starts_with("**");
    let english: Vec<String> = lines.iter().filter(|l| italic(l)).map(|l| l.trim_matches('*').to_string()).collect();
    let native: Vec<String> = lines.iter().filter(|l| !italic(l)).map(|l| (*l).to_string()).collect();
    match (spanish, native.is_empty(), english.is_empty()) {
        (true, false, _) | (false, true, _) => native,
        (false, false, true) => native,
        _ => english,
    }
}

/// A piece of a line of notes: plain, **bold**, `code`, or a link's label.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Span<'a> {
    Plain(&'a str),
    Bold(&'a str),
    Code(&'a str),
    Link(&'a str, &'a str),
}

/// Splits a line into its Markdown pieces: `**bold**`, `` `code` `` and `[label](https://…)`. What
/// is not closed stays as written, so odd text is never lost.
pub(crate) fn spans(line: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut plain_from = 0usize;
    let mut at = 0usize;
    let bytes = line.as_bytes();
    while at < bytes.len() {
        let tail = line.get(at..).unwrap_or("");
        let found = if tail.starts_with("**") {
            tail.get(2..).and_then(|t| t.find("**")).filter(|n| *n > 0).map(|n| (Span::Bold(tail.get(2..2 + n).unwrap_or("")), 2 + n + 2))
        } else if tail.starts_with('`') {
            tail.get(1..).and_then(|t| t.find('`')).filter(|n| *n > 0).map(|n| (Span::Code(tail.get(1..1 + n).unwrap_or("")), 1 + n + 1))
        } else if tail.starts_with('[') {
            tail.find("](").and_then(|close| {
                let label = tail.get(1..close)?;
                let after = tail.get(close + 2..)?;
                let end = after.find(')')?;
                (!label.contains('[') && !after.get(..end)?.contains(' '))
                    .then(|| (Span::Link(label, after.get(..end).unwrap_or("")), close + 2 + end + 1))
            })
        } else {
            None
        };
        match found {
            Some((span, len)) => {
                if plain_from < at {
                    out.push(Span::Plain(line.get(plain_from..at).unwrap_or("")));
                }
                out.push(span);
                at += len;
                plain_from = at;
            }
            None => {
                at += tail.chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    if plain_from < line.len() {
        out.push(Span::Plain(line.get(plain_from..).unwrap_or("")));
    }
    out
}

/// How many changes show before the rest folds away.
const SHOWN: usize = 6;

pub(crate) fn show(ui: &mut Ui, notes: &str) {
    show_scaled(ui, notes, 1.0);
}

/// The notes at `scale` times their usual size (the notes dialog's own zoom).
pub(crate) fn show_scaled(ui: &mut Ui, notes: &str, scale: f32) {
    ui.scope(|ui| {
        for font in ui.style_mut().text_styles.values_mut() {
            font.size *= scale;
        }
        show_lines(ui, notes, scale);
    });
}

/// The notes, short: the first few changes in full, the rest behind "Show N more changes", and the
/// link to the full changelog always in view.
fn show_lines(ui: &mut Ui, notes: &str, scale: f32) {
    let t = Tokens::get(ui.ctx());
    if notes.trim().is_empty() || normalise(notes, "en").is_empty() {
        ui.label(RichText::new(tl!("No notes were published with this release.")).color(t.muted));
        return;
    }
    let shown = normalise(notes, crate::i18n::current().code());
    let (changelog, lines): (Vec<Line<'_>>, Vec<Line<'_>>) = shown.lines().map(classify).partition(|l| full_changelog(l).is_some());
    let bullets = lines.iter().filter(|l| matches!(l, Line::Bullet(..))).count();
    let (head, rest) = lines.split_at(fold_at(&lines).unwrap_or(lines.len()));
    for line in head {
        draw(ui, line, &t, scale);
    }
    if !rest.is_empty() {
        let label = crate::i18n::fmt(tl!("Show {n} more changes"), &[("n", &bullets.saturating_sub(SHOWN).to_string())]);
        egui::CollapsingHeader::new(label).id_salt(ui.scope_id().with(("more-notes", notes.len()))).show(ui, |ui| {
            for line in rest {
                draw(ui, line, &t, scale);
            }
        });
    }
    for line in &changelog {
        if let Some(url) = full_changelog(line) {
            ui.add_space(6.0);
            ui.hyperlink_to(tl!("Full changelog"), url);
        }
    }
}

/// Where a long list folds: just before its (SHOWN + 1)th change. A short list is shown whole.
fn fold_at(lines: &[Line<'_>]) -> Option<usize> {
    let bullets = lines.iter().filter(|l| matches!(l, Line::Bullet(..))).count();
    if bullets <= SHOWN + 1 {
        return None;
    }
    lines.iter().enumerate().filter(|(_, l)| matches!(l, Line::Bullet(..))).nth(SHOWN).map(|(i, _)| i)
}

/// The address on a "**Full Changelog**: …" line, when it is a safe web link.
fn full_changelog<'a>(line: &Line<'a>) -> Option<&'a str> {
    let Line::Text(text) = line else { return None };
    text.strip_prefix("**Full Changelog**: ").map(str::trim).filter(|url| nodalia_launcher_core::release::safe_web_link(url))
}

/// A line with its Markdown drawn: bold in the heavier face, code in monospace, a link as its label
/// (a link whose address is a safe web link is clickable).
fn rich(ui: &mut Ui, text: &str, color: egui::Color32, t: &Tokens) {
    for span in spans(text) {
        match span {
            Span::Plain(s) => {
                ui.label(RichText::new(s).color(color));
            }
            Span::Bold(s) => {
                ui.label(RichText::new(s).family(crate::theme::semibold()).color(t.ink_strong));
            }
            Span::Code(s) => {
                ui.label(RichText::new(s).monospace().color(color));
            }
            Span::Link(label, url) if nodalia_launcher_core::release::safe_web_link(url) => {
                ui.hyperlink_to(label, url);
            }
            Span::Link(label, _) => {
                ui.label(RichText::new(label).color(color));
            }
        }
    }
}

fn draw(ui: &mut Ui, line: &Line<'_>, t: &Tokens, scale: f32) {
    match line {
        Line::Heading(h) => {
            ui.add_space(6.0);
            ui.label(crate::widgets::heading(h, 16.0 * scale, t.ink_strong));
        }
        Line::Bullet(text, link) => {
            // The author's handle is noise in a list of what changed.
            let text = text.rsplit_once(" by @").map_or(*text, |(change, _)| change);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("•").color(t.faint));
                rich(ui, text, t.ink, t);
                if let Some(url) = link.filter(|u| nodalia_launcher_core::release::safe_web_link(u)) {
                    let short = url.rsplit('/').next().map_or_else(|| tl!("link").to_string(), |n| format!("#{n}"));
                    ui.hyperlink_to(RichText::new(short).size(12.0 * scale), url);
                }
            });
        }
        Line::Text(text) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                rich(ui, text, t.muted, t);
            });
        }
        Line::Blank => {
            ui.add_space(2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_githubs_generated_notes() {
        assert_eq!(classify("## What's Changed"), Line::Heading("What's Changed"));
        assert_eq!(
            classify("* Add the Patch tool (#213) by @a in https://github.com/getnodalia/lienzo/pull/381"),
            Line::Bullet("Add the Patch tool (#213) by @a", Some("https://github.com/getnodalia/lienzo/pull/381"))
        );
        assert_eq!(classify("- plain bullet"), Line::Bullet("plain bullet", None));
        assert_eq!(classify("   "), Line::Blank);
        assert_eq!(classify("hello"), Line::Text("hello"));
    }

    const BILINGUAL: &str = "Celdas 0.1.9 da cabecera.\n*Celdas 0.1.9 gives a header.*\n\n> [!IMPORTANT]\n> Es temprano.\n> It is early.\n\n## Español\n\n### Novedades\n- **Editor:** cambia `vba.run`.\n\n## English\n\n### What's new\n- **Editor:** changes `vba.run`.\n\n## Descargas · Downloads\n\n| Sistema | Archivo |\n|---|---|\n| macOS | `x.dmg` |\n\n- **macOS:** no notarizada.\n";

    #[test]
    fn notes_show_the_readers_language_and_not_the_downloads() {
        let es = normalise(BILINGUAL, "es");
        assert_eq!(es, "Celdas 0.1.9 da cabecera.\n\nEs temprano.\n\n### Novedades\n- **Editor:** cambia `vba.run`.");
        let en = normalise(BILINGUAL, "fr");
        assert_eq!(en, "Celdas 0.1.9 gives a header.\n\nIt is early.\n\n### What's new\n- **Editor:** changes `vba.run`.");
        assert!(!es.contains("Descargas") && !en.contains("Downloads") && !en.contains("notarizada"));
    }

    #[test]
    fn notes_without_language_sections_keep_their_headings_and_lose_the_downloads() {
        let github = "## What's Changed\n* Add a tool in https://github.com/getnodalia/x/pull/1\n\n**Full Changelog**: https://github.com/getnodalia/x/compare/a...b";
        assert_eq!(normalise(github, "es"), github);
        let with_downloads = "## What's Changed\n* One\n\n## Downloads\n| a | b |";
        assert_eq!(normalise(with_downloads, "en"), "## What's Changed\n* One");
        assert_eq!(normalise("", "es"), "");
        assert_eq!(normalise("## Descargas\n| a |", "es"), "");
    }

    #[test]
    fn a_language_the_notes_lack_falls_back_to_the_other() {
        let only_es = "## Español\n- Algo";
        assert_eq!(normalise(only_es, "en"), "- Algo");
    }

    #[test]
    fn markdown_pieces_are_split_and_odd_text_is_kept() {
        assert_eq!(
            spans("**Fonts:** get `vba.run` from [here](https://x.y/z)."),
            [
                Span::Bold("Fonts:"),
                Span::Plain(" get "),
                Span::Code("vba.run"),
                Span::Plain(" from "),
                Span::Link("here", "https://x.y/z"),
                Span::Plain(".")
            ]
        );
        assert_eq!(spans("2 ** 3 and `open"), [Span::Plain("2 ** 3 and `open")]);
        assert_eq!(spans("[a](b c)"), [Span::Plain("[a](b c)")]);
        assert_eq!(spans("¡Hola! **ñandú**"), [Span::Plain("¡Hola! "), Span::Bold("ñandú")]);
        assert!(spans("").is_empty());
    }

    #[test]
    fn a_long_list_folds_after_six_changes_and_a_short_one_does_not() {
        let many: Vec<Line> = std::iter::once(Line::Heading("What's Changed")).chain((0..10).map(|_| Line::Bullet("change", None))).collect();
        // The heading, then six bullets: the seventh bullet (index 7) is the first one folded.
        assert_eq!(fold_at(&many), Some(7));
        let few: Vec<Line> = (0..7).map(|_| Line::Bullet("change", None)).collect();
        assert_eq!(fold_at(&few), None);
        assert_eq!(fold_at(&[]), None);
    }
}
