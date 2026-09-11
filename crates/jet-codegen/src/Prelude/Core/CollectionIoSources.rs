// Native file, stdin, and process collection-source adapters.
//
// Collections.rs owns target-neutral loop state and collection semantics. This
// fragment is emitted only after the filesystem/process runtime carriers and
// line-reading helpers are present.
impl JetLoopSource for JetFileReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::LinesFile => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(std::iter::from_fn(move || {
                    match jet_std_file_reader_read_line(&mut self) {
                        Ok(value) => value.map(|value| Box::new(value) as JetLoopAny),
                        Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                    }
                }))
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for &mut JetFileReader {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::LinesFile => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let mut values = Vec::new();
                loop {
                    match jet_std_file_reader_read_line(self) {
                        Ok(Some(value)) => values.push(value),
                        Ok(None) => break,
                        Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                    }
                }
                Box::new(
                    values
                        .into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for JetStdinReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::LinesStdin => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(std::iter::from_fn(move || {
                    match jet_std_io_stdin_read_line(&mut self) {
                        Ok(value) => value.map(|value| Box::new(value) as JetLoopAny),
                        Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                    }
                }))
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for &mut JetStdinReader {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::LinesStdin => {
                if by_value {
                    jet_loop_source_error(source_kind);
                }
                let mut values = Vec::new();
                loop {
                    match jet_std_io_stdin_read_line(self) {
                        Ok(Some(value)) => values.push(value),
                        Ok(None) => break,
                        Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                    }
                }
                Box::new(
                    values
                        .into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<R: std::io::Read + 'static> JetLoopSource
    for std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<R>>>>
{
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        _by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::LinesProcessStream => {
                let handle = std::rc::Rc::clone(&self);
                Box::new(std::iter::from_fn(move || {
                    match jet_process_stream_next_line(&handle) {
                        Ok(value) => value.map(|value| Box::new(value) as JetLoopAny),
                        Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                    }
                }))
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}
