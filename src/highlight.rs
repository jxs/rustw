// Copyright 2016 The Rustw Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Syntax highlighting.

use std::collections::HashMap;
use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str;
use std::time::Instant;

use rustdoc::html::highlight::{self, Classifier, Class};
use syntax::parse;
use syntax::parse::lexer::{self, TokenAndSpan};
use syntax::codemap::{CodeMap, Loc};

use analysis::{AnalysisHost, Span};

pub fn highlight<'a>(analysis: &'a AnalysisHost, project_path: &'a Path, file_name: String, file_text: String) -> String {
    debug!("highlight `{}` in `{}`", file_text, file_name);
    let sess = parse::ParseSess::new();
    let fm = sess.codemap().new_filemap(file_name.clone(), None, file_text);

    let mut out = Highlighter::new(analysis, project_path, sess.codemap());

    let t_start = Instant::now();

    let mut classifier = Classifier::new(lexer::StringReader::new(&sess.span_diagnostic, fm),
                                         sess.codemap());
    classifier.write_source(&mut out).unwrap();

    let time = t_start.elapsed();
    info!("Highlighting {} in {:.3}s", file_name, time.as_secs() as f64 + time.subsec_nanos() as f64 / 1_000_000_000.0);

    String::from_utf8_lossy(&out.buf).into_owned()
}

pub fn custom_highlight<H: highlight::Writer + GetBuf>(file_name: String, file_text: String, highlighter: &mut H) -> String {
    debug!("custom_highlight `{}` in `{}`", file_text, file_name);
    let sess = parse::ParseSess::new();
    let fm = sess.codemap().new_filemap(file_name.clone(), None, file_text);

    let mut classifier = Classifier::new(lexer::StringReader::new(&sess.span_diagnostic, fm),
                                         sess.codemap());
    classifier.write_source(highlighter).unwrap();

    String::from_utf8_lossy(highlighter.get_buf()).into_owned()
}

struct Highlighter<'a> {
    buf: Vec<u8>,
    analysis: &'a AnalysisHost,
    codemap: &'a CodeMap,
    project_path: &'a Path,
    path_cache: HashMap<String, PathBuf>,
}

impl<'a> Highlighter<'a> {
    fn new(analysis: &'a AnalysisHost, project_path: &'a Path, codemap: &'a CodeMap) -> Highlighter<'a> {
        Highlighter {
            buf: vec![],
            analysis: analysis,
            codemap: codemap,
            project_path: project_path,
            path_cache: HashMap::new(),
        }
    }

    fn get_link(&self, span: &Span) -> Option<String> {
        self.analysis.goto_def(span).ok().and_then(|def_span| {
            if span == &def_span {
                None
            } else {
                let file_name = Path::new(&def_span.file_name).strip_prefix(self.project_path)
                                                              .ok()
                                                              .unwrap_or(&def_span.file_name)
                                                              .to_str()
                                                              .unwrap();
                Some(format!("{}:{}:{}:{}:{}",
                             file_name,
                             def_span.line_start + 1,
                             def_span.column_start + 1,
                             def_span.line_end + 1,
                             def_span.column_end + 1))
            }
        })
    }

    fn write_span(buf: &mut Vec<u8>,
                  klass: Class,
                  text: String,
                  title: Option<String>,
                  extra_class: Option<String>,
                  id: Option<String>,
                  link: Option<String>,
                  doc_link: Option<String>,
                  src_link: Option<String>,
                  extra: Option<String>)
                  -> io::Result<()> {
        write!(buf, "<span class='{}", klass.rustdoc_class())?;
        if let Some(s) = extra_class {
            write!(buf, "{}", s)?;
        }
        if link.is_some() || doc_link.is_some() {
            write!(buf, " src_link")?;
        }
        write!(buf, "'")?;
        if let Some(s) = id {
            write!(buf, " id='{}'", s)?;
        }
        if let Some(s) = title {
            write!(buf, " title='")?;
            for c in s.chars() {
                push_char(buf, c)?;
            }
            write!(buf, "'")?;
        }
        if let Some(s) = doc_link {
            write!(buf, " doc_url='{}'", s)?;
        }
        if let Some(s) = src_link {
            write!(buf, " src_url='{}'", s)?;
        }
        if let Some(s) = link {
            write!(buf, " link='{}'", s)?;
        }
        if let Some(s) = extra {
            write!(buf, " {}", s)?;
        }
        write!(buf, ">{}</span>", text)
    }

    fn span_from_locs(&mut self, lo: &Loc, hi: &Loc) -> Span {
        let file_path = self.path_cache.entry(lo.file.name.clone()).or_insert_with(|| {
            Path::new(&lo.file.name).canonicalize().unwrap()
        });
        Span {
            file_name: file_path.clone(),
            line_start: lo.line as usize - 1,
            column_start: lo.col.0 as usize,
            line_end: hi.line as usize - 1,
            column_end: hi.col.0 as usize,
        }
    }
}

fn push_char(buf: &mut Vec<u8>, c: char) -> io::Result<()> {
    match c {
        '>' => write!(buf, "&gt;"),
        '<' => write!(buf, "&lt;"),
        '&' => write!(buf, "&amp;"),
        '\'' => write!(buf, "&#39;"),
        '"' => write!(buf, "&quot;"),
        '\n' => write!(buf, "<br>"),
        _ => write!(buf, "{}", c),
    }
}

impl<'a> highlight::Writer for Highlighter<'a> {
    fn enter_span(&mut self, klass: Class) -> io::Result<()> {
        write!(self.buf, "<span class='{}'>", klass.rustdoc_class())
    }

    fn exit_span(&mut self) -> io::Result<()> {
        write!(self.buf, "</span>")
    }

    fn string<T: Display>(&mut self, text: T, klass: Class, tas: Option<&TokenAndSpan>) -> io::Result<()> {
        let text = text.to_string();

        match klass {
            Class::None => write!(self.buf, "{}", text),
            Class::Ident => {
                match tas {
                    Some(t) => {
                        let lo = self.codemap.lookup_char_pos(t.sp.lo);
                        let hi = self.codemap.lookup_char_pos(t.sp.hi);
                        let span = &self.span_from_locs(&lo, &hi);
                        let ty = self.analysis.show_type(span).ok().and_then(|s| if s.is_empty() { None } else { Some(s) });
                        let docs = self.analysis.docs(span).ok().and_then(|s| if s.is_empty() { None } else { Some(s) });
                        let title = match (ty, docs) {
                            (Some(t), Some(d)) => Some(format!("{}\n\n{}", t, d)),
                            (Some(t), _) => Some(t),
                            (_, Some(d)) => Some(d),
                            (None, None) => None,
                        };
                        let mut link = self.get_link(span);
                        let doc_link = self.analysis.doc_url(span).ok();
                        let src_link = self.analysis.src_url(span).ok();

                        let css_class = match self.analysis.id(span) {
                            Ok(id) => {
                                if link.is_none() {
                                    link = Some(format!("search:{}", id));
                                }

                                Some(format!(" class_id class_id_{}", id))
                            }
                            Err(_) => None,
                        };


                        Highlighter::write_span(&mut self.buf, Class::Ident, text, title, css_class, None, link, doc_link, src_link, None)
                    }
                    None => Highlighter::write_span(&mut self.buf, Class::Ident, text, None, None, None, None, None, None, None),
                }
            }
            Class::Op if text == "*" => {
                match tas {
                    Some(t) => {
                        let lo = self.codemap.lookup_char_pos(t.sp.lo);
                        let hi = self.codemap.lookup_char_pos(t.sp.hi);
                        let span = &self.span_from_locs(&lo, &hi);
                        let title = self.analysis.show_type(span).ok();
                        let location = Some(format!("location='{}:{}''", lo.line, lo.col.0 + 1));
                        let css_class = Some(" glob".to_owned());

                        Highlighter::write_span(&mut self.buf, Class::Op, text, title, css_class, None, None, None, None, location)
                    }
                    None => Highlighter::write_span(&mut self.buf, Class::Op, text, None, None, None, None, None, None, None),
                }
            }
            klass => Highlighter::write_span(&mut self.buf, klass, text, None, None, None, None, None, None, None),
        }
    }
}

// Just does syntax highlighting, no fancy stuff.
pub struct BasicHighlighter {
    buf: Vec<u8>,
    spans: Vec<SpanSpan>,
}

struct SpanSpan {
    start_byte: u32,
    end_byte: u32,
    klass: String,
    id: String,
}

pub trait GetBuf {
    fn get_buf(&self) -> &[u8];
}

impl GetBuf for BasicHighlighter {
    fn get_buf(&self) -> &[u8] {
        &self.buf
    }    
}

impl BasicHighlighter {
    pub fn new() -> BasicHighlighter {
        BasicHighlighter {
            buf: vec![],
            spans: vec![],
        }
    }

    pub fn span(&mut self, start: u32, end: u32, klass: String, id: String) {
        self.spans.push(SpanSpan {
            start_byte: start,
            end_byte: end,
            klass: klass,
            id: id,
        });
    }
}

impl highlight::Writer for BasicHighlighter {
    fn enter_span(&mut self, klass: Class) -> io::Result<()> {
        write!(self.buf, "<span class='{}'>", klass.rustdoc_class())
    }

    fn exit_span(&mut self) -> io::Result<()> {
        write!(self.buf, "</span>")
    }

    fn string<T: Display>(&mut self, text: T, klass: Class, tas: Option<&TokenAndSpan>) -> io::Result<()> {
        // TODO use spans
        let text = text.to_string();

        let mut extra_class = None;
        let mut id = None;
        if let Some(tas) = tas {
            let lo = tas.sp.lo.0;
            let hi = tas.sp.hi.0;
            for s in &self.spans {
                if s.start_byte == lo && s.end_byte == hi {
                    extra_class = Some(s.klass.clone());
                    id = Some(s.id.clone());
                }
            }
        }

        Highlighter::write_span(&mut self.buf, klass, text, None, extra_class, id, None, None, None, None)
    }
}
