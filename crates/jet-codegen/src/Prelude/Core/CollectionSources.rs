// Native collection-source adapters shared by generated hosts.
//
// Collections.rs owns target-neutral loop state and collection semantics. This
// fragment is emitted after the scheduler and JetStd runtime carriers, so its
// implementations can preserve the native stream, channel, and reader paths.
impl<T: Send + 'static> JetLoopSource for jet_std::JetReceiver<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        _by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::ChannelReceiver => {
                let receiver = self;
                Box::new(std::iter::from_fn(move || match receiver.receive() {
                    Ok(value) => Some(Box::new(value) as JetLoopAny),
                    Err(_) => None,
                }))
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<T: Send + 'static> JetLoopSource for &mut jet_std::JetReceiver<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        _by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::ChannelReceiver => {
                let receiver = (*self).clone();
                Box::new(std::iter::from_fn(move || match receiver.receive() {
                    Ok(value) => Some(Box::new(value) as JetLoopAny),
                    Err(_) => None,
                }))
            }
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl<T: Send + 'static> JetLoopSource for jet_std::JetStream<T> {
    fn jet_loop_source(
        self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::Plain => {
                if !by_value {
                    jet_loop_source_error(source_kind);
                }
                Box::new(
                    self.into_iter()
                        .map(|value| Box::new(value) as JetLoopAny),
                )
            }
            JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::EncodingReader { .. }
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for jet_std::JSONReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::EncodingReader { reader_type } => match reader_type {
                "JSONReader" => {
                    if !by_value {
                        jet_loop_source_error(source_kind);
                    }
                    Box::new(std::iter::from_fn(move || {
                        match jet_enc_json_reader_next(&mut self) {
                            Ok(Ok(value)) => Some(Box::new(value) as JetLoopAny),
                            Ok(Err(_)) => None,
                            Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                        }
                    }))
                }
                _ => jet_loop_source_error(source_kind),
            },
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for jet_std::JSONLReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::EncodingReader { reader_type } => match reader_type {
                "JSONLReader" => {
                    if !by_value {
                        jet_loop_source_error(source_kind);
                    }
                    Box::new(std::iter::from_fn(move || {
                        match jet_enc_jsonl_reader_next(&mut self) {
                            Ok(Ok(value)) => Some(Box::new(value) as JetLoopAny),
                            Ok(Err(_)) => None,
                            Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                        }
                    }))
                }
                _ => jet_loop_source_error(source_kind),
            },
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for jet_std::CSVReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::EncodingReader { reader_type } => match reader_type {
                "CSVReader" => {
                    if !by_value {
                        jet_loop_source_error(source_kind);
                    }
                    Box::new(std::iter::from_fn(move || {
                        match jet_enc_csv_reader_next(&mut self) {
                            Ok(Ok(value)) => Some(Box::new(value) as JetLoopAny),
                            Ok(Err(_)) => None,
                            Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                        }
                    }))
                }
                _ => jet_loop_source_error(source_kind),
            },
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for jet_std::XMLReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::EncodingReader { reader_type } => match reader_type {
                "XMLReader" => {
                    if !by_value {
                        jet_loop_source_error(source_kind);
                    }
                    Box::new(std::iter::from_fn(move || {
                        match jet_enc_xml_reader_next(&mut self) {
                            Ok(Ok(value)) => Some(Box::new(value) as JetLoopAny),
                            Ok(Err(_)) => None,
                            Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                        }
                    }))
                }
                _ => jet_loop_source_error(source_kind),
            },
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}

impl JetLoopSource for jet_std::CBORReader {
    fn jet_loop_source(
        mut self,
        source_kind: JetLoopSourceKind,
        by_value: bool,
    ) -> Box<dyn Iterator<Item = JetLoopAny>> {
        match source_kind {
            JetLoopSourceKind::EncodingReader { reader_type } => match reader_type {
                "CBORReader" => {
                    if !by_value {
                        jet_loop_source_error(source_kind);
                    }
                    Box::new(std::iter::from_fn(move || {
                        match jet_enc_cbor_reader_next(&mut self) {
                            Ok(Ok(value)) => Some(Box::new(value) as JetLoopAny),
                            Ok(Err(_)) => None,
                            Err(error) => jet_panic("<core.prelude>", 0, &error.jet_show()),
                        }
                    }))
                }
                _ => jet_loop_source_error(source_kind),
            },
            JetLoopSourceKind::Plain
            | JetLoopSourceKind::Chars
            | JetLoopSourceKind::LinesFile
            | JetLoopSourceKind::LinesStdin
            | JetLoopSourceKind::LinesProcessStream
            | JetLoopSourceKind::ChannelReceiver
            | JetLoopSourceKind::Iterable { .. } => jet_loop_source_error(source_kind),
        }
    }
}
