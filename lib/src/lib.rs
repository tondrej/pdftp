pub mod error;

use crate::error::Error;
use regex::{Captures, Match, Regex};
use std::cell::{Cell, RefCell};
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct Conditional {
    name: String,
    on: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StreamInfo {
    pub length: usize,
    pub raw: bool,
}

impl StreamInfo {
    pub fn new() -> Self {
        Self {
            length: 0,
            raw: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum StreamState {
    FindStream,
    FindEndStream,
}

#[derive(Debug, Clone)]
pub struct ObjectEntry {
    objnum: usize,
    gennum: usize,
    offset: Option<usize>,
    alias: String,
}

impl ObjectEntry {
    pub fn new(objnum: usize, gennum: usize, offset: Option<usize>, alias: String) -> Self {
        Self {
            objnum,
            gennum,
            offset,
            alias,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pass {
    Initial,
    ReadInput,
    WriteOutput,
}

type RegexCallback<W> = fn(&mut Processor<W>, Captures, &[Match]) -> Result<(), Error>;

pub struct Processor<W: Write> {
    pass: Rc<Cell<Pass>>,
    reader: BufReader<File>,
    writer: Rc<RefCell<BufWriter<W>>>,
    conditionals: Rc<RefCell<Vec<Conditional>>>,
    variables: Rc<RefCell<HashMap<String, String>>>,
    filename: String,
    includes: Rc<RefCell<Vec<String>>>,
    stream_infos: Rc<RefCell<Vec<StreamInfo>>>,
    stream_state: Rc<Cell<StreamState>>,
    aliases: Rc<RefCell<HashMap<String, usize>>>,
    objects: Rc<RefCell<HashMap<usize, ObjectEntry>>>,
    max_objnum: Rc<Cell<usize>>,
    bytes_written: Rc<Cell<usize>>,
    xref_offset: Rc<Cell<usize>>,
}

impl<W: Write> Processor<W> {
    pub fn new(
        input_filename: impl AsRef<Path>,
        writer: BufWriter<W>,
        variables: Option<Vec<(String, String)>>,
    ) -> Self {
        let variables: HashMap<String, String> = match variables {
            Some(variables) => variables
                .iter()
                .map(|(name, value)| (String::from(name), String::from(value)))
                .collect(),
            None => HashMap::new(),
        };
        let reader = BufReader::new(File::open(&input_filename).unwrap());
        Self {
            pass: Rc::new(Cell::new(Pass::Initial)),
            reader,
            writer: Rc::new(RefCell::new(writer)),
            conditionals: Rc::new(RefCell::new(vec![])),
            variables: Rc::new(RefCell::new(variables)),
            filename: input_filename.as_ref().to_str().unwrap().to_owned(),
            includes: Rc::new(RefCell::new(vec![])),
            stream_infos: Rc::new(RefCell::new(vec![])),
            stream_state: Rc::new(Cell::new(StreamState::FindStream)),
            aliases: Rc::new(RefCell::new(HashMap::new())),
            objects: Rc::new(RefCell::new(HashMap::new())),
            max_objnum: Rc::new(Cell::new(0usize)),
            bytes_written: Rc::new(Cell::new(0usize)),
            xref_offset: Rc::new(Cell::new(0usize)),
        }
    }

    pub fn from(other: &Self, input_filename: impl AsRef<Path>) -> Self {
        let reader = BufReader::new(File::open(&input_filename).unwrap());
        let input_filename = input_filename.as_ref().to_str().unwrap().to_owned();
        let includes = other.includes.clone();
        includes.borrow_mut().push(String::from(&input_filename));
        Self {
            pass: other.pass.clone(),
            reader,
            writer: other.writer.clone(),
            conditionals: other.conditionals.clone(),
            variables: other.variables.clone(),
            filename: input_filename,
            includes: other.includes.clone(),
            stream_infos: other.stream_infos.clone(),
            stream_state: other.stream_state.clone(),
            aliases: other.aliases.clone(),
            objects: other.objects.clone(),
            max_objnum: other.max_objnum.clone(),
            bytes_written: other.bytes_written.clone(),
            xref_offset: other.xref_offset.clone(),
        }
    }

    fn obj_entry_by_alias(&self, alias: &str) -> Option<ObjectEntry> {
        if let Some(objnum) = self.aliases.borrow().get(alias)
            && let Some(entry) = self.objects.borrow().get(objnum)
        {
            return Some(entry.clone());
        }
        None
    }

    pub fn execute(&mut self) -> Result<(), Error> {
        assert_eq!(Pass::Initial, self.pass.get());

        self.pass.set(Pass::ReadInput);
        self.process_all_lines()?;
        if !self.conditionals.borrow().is_empty() {
            return Err(Error::Conditional);
        }

        self.reader.seek(SeekFrom::Start(0))?;
        self.pass.set(Pass::WriteOutput);
        self.process_all_lines()
    }

    fn process_all_lines(&mut self) -> Result<(), Error> {
        let mut buf = String::new();
        loop {
            let bytes_read = self.reader.read_line(&mut buf)?;
            if bytes_read == 0 {
                break;
            }
            let mut line = buf.as_str();
            if line.ends_with("\n") {
                line = &line[..line.len() - 1];
            }
            self.process_single_line(line, true)?;
            buf.clear();
        }
        Ok(())
    }

    fn process_single_line(&mut self, line: &str, append_newline: bool) -> Result<(), Error> {
        match self.pass.get() {
            Pass::ReadInput => self.preprocess_line(line),
            Pass::WriteOutput => self.process_line(line, append_newline),
            _ => Err(Error::Preprocessing),
        }
    }

    fn expand_variables(&mut self, line: &str) -> Result<String, Error> {
        let re = Regex::new(r"\{\{(var\s+(?<name>\w*))}}").map_err(|_| Error::Regex)?;
        let mut name_not_found: Option<String> = None;
        let result = String::from(re.replace(line, |captures: &Captures| {
            let matches: Vec<Match> = captures.iter().flatten().collect();
            let match_count = matches.len();
            if match_count == 3 {
                let name = &captures["name"];
                if let Some(value) = self.variables.borrow().get(name) {
                    return String::from(value);
                } else if name_not_found.is_none() {
                    name_not_found = Some(String::from(name));
                }
            }
            String::new()
        }));

        match name_not_found {
            Some(_) => Err(Error::Variable),
            None => Ok(result),
        }
    }

    fn add_xref_entry(
        &mut self,
        objnum: usize,
        gennum: usize,
        offset: Option<usize>,
        alias: Option<String>,
    ) {
        let mut objnum = objnum;
        let alias = match alias {
            Some(alias) => {
                if let Some(&num) = self.aliases.borrow().get(&alias) {
                    objnum = num;
                }
                alias
            }
            None => format!("obj_{objnum}_{gennum}"),
        };
        if objnum == 0 {
            objnum = self.max_objnum.get() + 1;
        }
        self.objects
            .borrow_mut()
            .entry(objnum)
            .and_modify(|o| {
                o.gennum = gennum;
                o.alias = alias.clone();
                o.offset = offset;
            })
            .or_insert(ObjectEntry {
                objnum,
                gennum,
                offset,
                alias: alias.clone(),
            });
        self.aliases.borrow_mut().entry(alias).or_insert(objnum);
        if objnum > self.max_objnum.get() {
            self.max_objnum.set(objnum);
        }
    }

    fn parse_object(&mut self, captures: Captures, matches: &[Match]) -> Result<(), Error> {
        let offset: Option<usize> = match self.pass.get() {
            Pass::WriteOutput => Some(self.bytes_written.get()),
            _ => None,
        };

        match matches.len() {
            5 => {
                if let (Ok(objnum), Ok(gennum)) = (
                    captures["objnum"].parse::<usize>(),
                    captures["gennum"].parse::<usize>(),
                ) {
                    self.add_xref_entry(objnum, gennum, offset, None);
                    if self.pass.get() == Pass::WriteOutput {
                        let entry = format!("{} {} obj\n", objnum, gennum);
                        self.write(entry.as_bytes())?;
                    }
                    Ok(())
                } else {
                    Err(Error::Parsing)
                }
            }
            4 => {
                let alias = String::from(&captures["alias"]);
                let objnum = if let Some(&objnum) = self.aliases.borrow_mut().get(&alias) {
                    self.objects.borrow_mut().entry(objnum).and_modify(|o| {
                        o.objnum = objnum;
                        o.gennum = 0;
                    });
                    objnum
                } else {
                    0usize
                };
                self.add_xref_entry(objnum, 0, offset, Some(alias));
                if self.pass.get() == Pass::WriteOutput {
                    let entry = format!("{} {} obj\n", objnum, 0);
                    self.write(entry.as_bytes())?;
                }
                Ok(())
            }
            _ => Err(Error::Parsing),
        }
    }

    fn parse_objects(&mut self, line: &str) -> Result<bool, Error> {
        self.process_regex(
            line,
            r"\{\{((object\s+(?<objnum>\d+)\s+(?<gennum>\d+))|(object\s+(?<alias>\w+)))}}\s*",
            Self::parse_object,
            true,
        )
    }

    fn preprocess_line(&mut self, line: &str) -> Result<(), Error> {
        assert_eq!(Pass::ReadInput, self.pass.get());

        if self.process_conditionals(line)? {
            return Ok(());
        }
        if !self.check_conditionals() {
            return Ok(());
        }
        let line = self.expand_variables(line)?;
        if self.process_includes(&line)? {
            return Ok(());
        }
        if self.process_includes_raw(&line)? {
            return Ok(());
        }

        match self.stream_state.get() {
            StreamState::FindStream => {
                if line.trim() == "stream" {
                    if self.stream_state.get() != StreamState::FindStream {
                        return Err(Error::Stream);
                    }
                    self.stream_state.set(StreamState::FindEndStream);
                    self.stream_infos.borrow_mut().push(StreamInfo::new());
                    return Ok(());
                }
            }
            StreamState::FindEndStream => {
                if !line.trim().is_empty() {
                    let mut infos = self.stream_infos.borrow_mut();
                    let len = infos.len();
                    if line.trim() == "endstream" {
                        if !infos[len - 1].raw {
                            infos[len - 1].length -= 1;
                        }
                        self.stream_state.set(StreamState::FindStream);
                    } else if !infos[len - 1].raw {
                        infos[len - 1].length += line.len() + 1;
                    }
                }
                return Ok(());
            }
        }
        self.parse_objects(&line)?;
        Ok(())
    }

    fn include_raw(&mut self, filename: &str) -> Result<(), Error> {
        assert_eq!(Pass::WriteOutput, self.pass.get());

        let mut reader = BufReader::new(File::open(filename)?);
        let mut writer = self.writer.borrow_mut();
        let bytes_written = io::copy(&mut reader, &mut writer as &mut BufWriter<W>)? as usize;
        self.bytes_written.update(|n| n + bytes_written);
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> Result<(), Error> {
        assert_eq!(Pass::WriteOutput, self.pass.get());

        let mut writer = self.writer.borrow_mut();
        let bytes_written = writer.write(buf)?;
        self.bytes_written.update(|n| n + bytes_written);
        writer.flush()?;
        Ok(())
    }

    fn generate_xref_table(&mut self) -> Result<(), Error> {
        assert_eq!(Pass::WriteOutput, self.pass.get());

        self.write(b"xref\n")?;
        self.write(format!("0 {}\n", self.max_objnum.get() + 1).as_bytes())?;
        for i in 0..self.max_objnum.get() + 1 {
            if let Some(o) = self.objects.clone().borrow_mut().get(&i)
                && let Some(offset) = o.offset
            {
                self.write(format!("{:010} {:05} n \n", offset, o.gennum).as_bytes())?;
            } else {
                self.write(b"0000000000 65535 f \n")?;
            }
        }
        Ok(())
    }

    fn expand_reference(&mut self, captures: Captures, matches: &[Match]) -> Result<(), Error> {
        assert_eq!(Pass::WriteOutput, self.pass.get());
        match matches.len() {
            3 => {
                let name = &captures["name"];
                if let Some(entry) = self.obj_entry_by_alias(name) {
                    let reference = format!("{} {} R", entry.objnum, entry.gennum);
                    self.write(reference.as_bytes())
                } else {
                    Err(Error::Reference)
                }
            }
            _ => Err(Error::Regex),
        }
    }

    fn expand_references(&mut self, line: &str) -> Result<bool, Error> {
        self.process_regex(
            line,
            r"\{\{(ref\s+(?<name>\w+))}}",
            Self::expand_reference,
            true,
        )
    }

    fn process_line(&mut self, line: &str, append_newline: bool) -> Result<(), Error> {
        assert_eq!(Pass::WriteOutput, self.pass.get());

        if self.process_conditionals(line)? {
            return Ok(());
        }
        if !self.check_conditionals() {
            return Ok(());
        }
        let line = self.expand_variables(line)?;
        if self.process_includes(&line)? {
            return Ok(());
        }
        if self.process_includes_raw(&line)? {
            return Ok(());
        }
        if line.contains("{{header}}") {
            self.write(b"%PDF-1.7\n")?;
            self.write(&[b'%', 0xa0, 0xf2, 0xa4, 0xf4, b'\n'])?;
        } else if line.contains("{{streamlen}}") {
            let value = self.stream_infos.borrow().first().unwrap().length;
            let line = line.replace("{{streamlen}}", format!("/Length {}\n", value).as_str());
            self.write(line.as_bytes())?;
            self.stream_infos.borrow_mut().remove(0);
        } else if line.contains("{{xref}}") {
            self.xref_offset.set(self.bytes_written.get());
            self.generate_xref_table()?;
        } else if line.contains("{{trailer}}") {
            let catalog = match self.obj_entry_by_alias("catalog") {
                Some(catalog) => catalog,
                None => return Err(Error::Catalog),
            };
            let trailer = format!(
                "trailer <<\n  /Root {} {} R\n  /Size {}\n>>\n",
                catalog.objnum,
                catalog.gennum,
                self.max_objnum.get() + 1
            );
            let line = line.replace("{{trailer}}", trailer.as_str());
            self.write(line.as_bytes())?;
        } else if line.contains("{{trailersize}}") {
            let trailersize = format!("/Size {}", self.max_objnum.get() + 1);
            let line = line.replace("{{trailersize}}", trailersize.as_str());
            self.write(line.as_bytes())?;
        } else if line.contains("{{startxref}}") {
            let startxref = format!("startxref\n{}\n", self.xref_offset.get());
            let line = line.replace("{{startxref}}", startxref.as_str());
            self.write(line.as_bytes())?;
        } else if line.contains("{{object ") {
            self.parse_objects(line.as_str())?;
        } else if line.contains("{{ref ") {
            self.expand_references(line.as_str())?;
        } else {
            self.write(line.as_bytes())?;
            if append_newline && !line.ends_with("\n") {
                self.write(b"\n")?;
            }
        }
        Ok(())
    }

    fn resolve_filename(&self, filename: &str) -> Result<String, Error> {
        // including absolute path?
        if Path::is_absolute(filename.as_ref()) {
            return Ok(String::from(filename));
        }

        // including path relative to current filename
        let mut path = {
            let mut path = PathBuf::from(&self.filename);
            path.pop();
            path
        };
        path.push(filename);
        path.to_str().map(String::from).ok_or(Error::Io)
    }

    fn process_include(&mut self, captures: Captures, _matches: &[Match]) -> Result<(), Error> {
        let filename = self.resolve_filename(&captures["filename"])?;
        let mut include_processor = Self::from(self, filename);
        include_processor.process_all_lines()
    }

    fn process_includes(&mut self, line: &str) -> Result<bool, Error> {
        self.process_regex(
            line,
            r"\{\{(include\s+(?<filename>[\w/\.-]*(?:\.\w*)?))}}",
            Self::process_include,
            false,
        )
    }

    fn process_include_raw(&mut self, captures: Captures, _matches: &[Match]) -> Result<(), Error> {
        let filename = self.resolve_filename(&captures["filename"])?;
        match self.pass.get() {
            Pass::ReadInput => {
                let file = File::open(filename)?;
                let mut infos = self.stream_infos.borrow_mut();
                let len = infos.len();
                infos[len - 1].raw = true;
                infos[len - 1].length = file.metadata()?.len() as usize;
                Ok(())
            }
            Pass::WriteOutput => self.include_raw(&filename),
            _ => Err(Error::Preprocessing),
        }
    }

    fn process_includes_raw(&mut self, line: &str) -> Result<bool, Error> {
        self.process_regex(
            line,
            r"\{\{(includeraw\s+(?<filename>[\w/\.-]*(?:\.\w*)?))}}",
            Self::process_include_raw,
            true,
        )
    }

    fn check_conditionals(&mut self) -> bool {
        self.conditionals.borrow().iter().all(|conditional| {
            conditional.on == self.variables.borrow().contains_key(&conditional.name)
        })
    }

    fn process_regex(
        &mut self,
        line: &str,
        regex: &str,
        callback: RegexCallback<W>,
        append_newline: bool,
    ) -> Result<bool, Error> {
        let mut match_found = false;
        let mut buf = line;
        let re = Regex::new(regex).map_err(|_| Error::Regex)?;

        while let Some(captures) = re.captures(buf) {
            match_found = true;

            let matches: Vec<Match> = captures.iter().flatten().collect();

            // process text before first match
            let part = &buf[..matches[0].start()];
            if !part.trim().is_empty() {
                self.process_single_line(part, false)?;
            }

            callback(self, captures, &matches)?;

            buf = &buf[matches[0].end()..];
        }

        // process text after last match
        if match_found {
            self.process_single_line(buf, append_newline)?;
        }
        Ok(match_found)
    }

    fn process_conditional(&mut self, captures: Captures, matches: &[Match]) -> Result<(), Error> {
        match matches.len() {
            3 if matches[1].as_str() == "if" => {
                let name = &captures["name"];
                self.conditionals.borrow_mut().push(Conditional {
                    name: name.to_owned(),
                    on: true,
                });
                Ok(())
            }
            2 if matches[1].as_str() == "else" => {
                if self.conditionals.borrow().is_empty() {
                    return Err(Error::Conditional); // {{else}} without {{if}}
                }
                let len = self.conditionals.borrow().len();
                if !self.conditionals.borrow()[len - 1].on {
                    return Err(Error::Conditional); // multiple {{else}}
                }
                self.conditionals.borrow_mut()[len - 1].on = false;
                Ok(())
            }
            2 if matches[1].as_str() == "endif" => {
                if self.conditionals.borrow().is_empty() {
                    return Err(Error::Conditional); // {{endif}} without {{if}}
                }
                let len = self.conditionals.borrow().len();
                self.conditionals.borrow_mut().remove(len - 1);
                Ok(())
            }
            _ => Err(Error::Conditional),
        }
    }

    fn process_conditionals(&mut self, line: &str) -> Result<bool, Error> {
        self.process_regex(
            line,
            r"\{\{(?:(if)\s+(?<name>\w*)|(else)|(endif))}}",
            Self::process_conditional,
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert!(true);
    }
}
