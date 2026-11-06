//! The Wikibase entity JSON dump: a JSON array with one entity per line, as
//! `dumpJson.php` writes it and Wikidata publishes it. Read line by line, so a 100 GB
//! dump streams; the brackets and the trailing commas are the format's and are dropped.

use std::io::BufRead;
use std::path::Path;

use scatter_wikibase_model::entity::{Entity, ParsedEntity};

use crate::DumpError;

/// A streaming reader over an entity JSON dump.
pub struct JsonDump<R: BufRead> {
    reader: R,
    line: usize,
    buf: String,
}

impl JsonDump<Box<dyn BufRead + Send>> {
    /// Opens a dump file, decompressing by extension.
    pub fn open(path: &Path) -> Result<Self, DumpError> {
        Ok(Self::new(crate::open(path)?))
    }
}

impl<R: BufRead> JsonDump<R> {
    /// Over a reader.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line: 0,
            buf: String::new(),
        }
    }

    /// The 1-based line the last entity came from.
    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }
}

impl<R: BufRead> Iterator for JsonDump<R> {
    type Item = Result<ParsedEntity, DumpError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.buf.clear();
            match self.reader.read_line(&mut self.buf) {
                Ok(0) => return None,
                Ok(_) => {}
                Err(e) => return Some(Err(e.into())),
            }
            self.line += 1;
            if let Some(entity) = parse_line(self.line, &self.buf) {
                return Some(entity);
            }
        }
    }
}

/// One line of the dump: its entity, or `None` for the array's brackets and blank lines.
/// `line` is 1-based, for the error. The reader's step, public so that a caller can parse
/// lines it read itself on several threads (a bootstrap load, 0013 §9).
#[must_use]
pub fn parse_line(line: usize, text: &str) -> Option<Result<ParsedEntity, DumpError>> {
    let mut text = text.trim();
    if text == "[" || text == "]" || text.is_empty() {
        return None;
    }
    if let Some(t) = text.strip_suffix(',') {
        text = t;
    }
    if text == "[" || text == "]" {
        return None;
    }
    Some(Entity::from_json(text).map_err(|e| DumpError::Entity {
        line,
        message: e.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_array_form() {
        let text = concat!(
            "[\n",
            r#"{"type":"item","id":"Q1","labels":{"en":{"language":"en","value":"one"}},"claims":{},"lastrevid":5,"modified":"2026-01-01T00:00:00Z"},"#,
            "\n",
            r#"{"type":"property","id":"P1","datatype":"string","labels":{},"claims":{}}"#,
            "\n]\n"
        );
        let dump = JsonDump::new(text.as_bytes());
        let entities: Vec<ParsedEntity> = dump.map(Result::unwrap).collect();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].entity.id.as_str(), "Q1");
        assert_eq!(entities[0].page.as_ref().unwrap().lastrevid, Some(5));
        assert_eq!(entities[1].entity.id.as_str(), "P1");
        let mut bad = JsonDump::new("[\nnot json\n]".as_bytes());
        assert!(matches!(
            bad.next(),
            Some(Err(DumpError::Entity { line: 2, .. }))
        ));
    }
}
