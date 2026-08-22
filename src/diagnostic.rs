use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

/// Source locations for instructions, kept separate from the public AST.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgramOrigins {
    instruction_spans: HashMap<String, Vec<Span>>,
    declaration_spans: HashMap<String, Span>,
    label_spans: HashMap<String, Span>,
    sources: SourceMap,
}

impl ProgramOrigins {
    pub(crate) fn with_sources(mut self, sources: SourceMap) -> Self {
        self.sources = sources;
        self
    }

    pub fn sources(&self) -> &SourceMap {
        &self.sources
    }

    pub(crate) fn record_instruction(&mut self, label: &str, span: Span) {
        self.instruction_spans
            .entry(label.to_owned())
            .or_default()
            .push(span);
    }

    pub fn instruction_span(&self, label: &str, index: usize) -> Option<Span> {
        self.instruction_spans
            .get(label)
            .and_then(|spans| spans.get(index))
            .copied()
    }

    pub fn declaration_span(&self, name: &str) -> Option<Span> {
        self.declaration_spans.get(name).copied()
    }

    pub fn label_span(&self, label: &str) -> Option<Span> {
        self.label_spans.get(label).copied()
    }

    pub(crate) fn merge_label_origins(
        &mut self,
        other: &ProgramOrigins,
        label_map: &HashMap<String, String>,
    ) {
        let source_ids = self.sources.extend(&other.sources);
        for (label, spans) in &other.instruction_spans {
            let merged_label = label_map.get(label).unwrap_or(label);
            let spans = spans
                .iter()
                .map(|span| Span::new(source_ids[&span.source], span.start, span.end))
                .collect();
            self.instruction_spans.insert(merged_label.clone(), spans);
        }
        for (name, span) in &other.declaration_spans {
            let merged_name = label_map.get(name).unwrap_or(name);
            self.declaration_spans.insert(
                merged_name.clone(),
                Span::new(source_ids[&span.source], span.start, span.end),
            );
        }
        for (label, span) in &other.label_spans {
            let merged_label = label_map.get(label).unwrap_or(label);
            self.label_spans.insert(
                merged_label.clone(),
                Span::new(source_ids[&span.source], span.start, span.end),
            );
        }
    }

    pub(crate) fn record_declaration(&mut self, name: &str, span: Span) {
        self.declaration_spans.insert(name.to_owned(), span);
    }

    pub(crate) fn record_label(&mut self, label: &str, span: Span) {
        self.label_spans.insert(label.to_owned(), span);
    }
}

impl Span {
    pub fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let id = SourceId(self.files.len());
        self.files.push(SourceFile {
            name: name.into(),
            text: text.into(),
        });
        id
    }

    pub fn get(&self, source: SourceId) -> Option<&SourceFile> {
        self.files.get(source.0)
    }

    pub(crate) fn extend(&mut self, other: &SourceMap) -> HashMap<SourceId, SourceId> {
        let mut source_ids = HashMap::new();
        for (index, file) in other.files.iter().enumerate() {
            let source = SourceId(index);
            let mapped = self.add(file.name.clone(), file.text.clone());
            source_ids.insert(source, mapped);
        }
        source_ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Option<Span>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
            notes: Vec::new(),
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn render(&self, sources: &SourceMap) -> String {
        let Some(span) = self.span else {
            return self.message.clone();
        };
        let Some(file) = sources.get(span.source) else {
            return self.message.clone();
        };

        let mut start = span.start.min(file.text.len());
        while start > 0 && !file.text.is_char_boundary(start) {
            start -= 1;
        }
        let line_start = file.text[..start].rfind('\n').map_or(0, |index| index + 1);
        let line_end = file.text[start..]
            .find('\n')
            .map_or(file.text.len(), |index| start + index);
        let line_number = file.text[..line_start]
            .bytes()
            .filter(|&byte| byte == b'\n')
            .count()
            + 1;
        let column = file.text[line_start..start].chars().count() + 1;
        let source_line = &file.text[line_start..line_end];
        let marker_end = span.end.min(line_end).max(start);
        let width = file.text[start..marker_end].chars().count().max(1);

        let mut rendered = format!(
            "error: {}\n --> {}:{}:{}\n  |\n{} | {}\n  | {}{}",
            self.message,
            file.name,
            line_number,
            column,
            line_number,
            source_line,
            " ".repeat(column.saturating_sub(1)),
            format!("^{}", "~".repeat(width.saturating_sub(1))),
        );
        for note in &self.notes {
            rendered.push_str(&format!("\nnote: {note}"));
        }
        rendered
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
