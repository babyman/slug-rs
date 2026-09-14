use std::{
    env, fs,
    io::{self, BufRead, BufReader, IsTerminal, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio},
};

use rustyline::{DefaultEditor, error::ReadlineError};
use serde_json::{Value, json};
use slug_vm::interactive::{
    Diagnostic, DiagnosticCategory, Event, IncomingMessage, PROTOCOL_VERSION, Request, Response,
};

fn main() -> ExitCode {
    let mut arguments = env::args_os();
    let executable = arguments.next().unwrap_or_else(|| "slug-repl".into());
    let startup_path = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        eprintln!("Usage: {} [session.slug]", executable.to_string_lossy());
        return ExitCode::from(1);
    }
    let startup_source = match startup_path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(source) => Some(source),
            Err(error) => {
                eprintln!("slug-repl: cannot read {}: {error}", path.display());
                return ExitCode::from(1);
            }
        },
        None => None,
    };
    let stdin = io::stdin();
    let mut output = io::stdout().lock();
    let mut errors = io::stderr().lock();
    if stdin.is_terminal() {
        run_interactive(&mut output, &mut errors, startup_source.as_deref())
    } else {
        run(
            &mut stdin.lock(),
            &mut output,
            &mut errors,
            startup_source.as_deref(),
        )
    }
}

enum ReadSubmission {
    Exit,
    Source(String),
}

fn run(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    errors: &mut dyn Write,
    startup_source: Option<&str>,
) -> ExitCode {
    let mut reader = BufferedInput { input };
    run_with_reader(&mut reader, output, errors, startup_source)
}

fn run_interactive(
    output: &mut dyn Write,
    errors: &mut dyn Write,
    startup_source: Option<&str>,
) -> ExitCode {
    let editor = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(error) => {
            let _ = writeln!(
                errors,
                "slug-repl: cannot initialize terminal input: {error}"
            );
            return ExitCode::from(1);
        }
    };
    let mut reader = TerminalInput { editor };
    run_with_reader(&mut reader, output, errors, startup_source)
}

fn run_with_reader(
    reader: &mut dyn SubmissionReader,
    output: &mut dyn Write,
    errors: &mut dyn Write,
    startup_source: Option<&str>,
) -> ExitCode {
    let mut server = match ServerProcess::spawn() {
        Ok(server) => server,
        Err(error) => {
            let _ = writeln!(errors, "slug-repl: cannot start slug-server: {error}");
            return ExitCode::from(1);
        }
    };
    let result = run_session(&mut server, reader, output, errors, startup_source);
    let shutdown = server.finish();
    if let Err(error) = shutdown {
        let _ = writeln!(errors, "slug-repl: slug-server shutdown failed: {error}");
        return ExitCode::from(1);
    }
    result
}

fn run_session(
    server: &mut ServerProcess,
    input: &mut dyn SubmissionReader,
    output: &mut dyn Write,
    errors: &mut dyn Write,
    startup_source: Option<&str>,
) -> ExitCode {
    let mut request_id = 1;
    let initialized = match request(
        server,
        request_id,
        "initialize",
        None,
        json!({
            "protocol": PROTOCOL_VERSION,
        }),
        output,
        errors,
    ) {
        Ok(response) => response,
        Err(error) => return render_transport_error(&error, errors),
    };
    request_id += 1;
    let Some(()) = render_startup_response(&initialized, errors) else {
        return ExitCode::from(1);
    };
    let opened = match request(
        server,
        request_id,
        "session.open",
        None,
        Value::Null,
        output,
        errors,
    ) {
        Ok(response) => response,
        Err(error) => return render_transport_error(&error, errors),
    };
    request_id += 1;
    let Some(session) = opened
        .result
        .as_ref()
        .and_then(|result| result.get("session"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        render_response_error(&opened, errors);
        return ExitCode::from(1);
    };

    if let Some(source) = startup_source
        && let Err(exit_code) =
            submit_startup_source(server, request_id, &session, source, output, errors)
    {
        return exit_code;
    }
    if startup_source.is_some() {
        request_id += 1;
    }

    if writeln!(output, "Slug REPL\n").is_err() {
        return ExitCode::from(1);
    }
    let mut continuing = false;
    loop {
        let source = match input.read_submission(output, continuing) {
            Ok(ReadSubmission::Exit) => break,
            Ok(ReadSubmission::Source(source)) => source,
            Err(error) => {
                let _ = writeln!(errors, "input error: {error}");
                return ExitCode::from(1);
            }
        };
        let response = match request(
            server,
            request_id,
            "submit",
            Some(&session),
            json!({ "source": source }),
            output,
            errors,
        ) {
            Ok(response) => response,
            Err(error) => return render_transport_error(&error, errors),
        };
        request_id += 1;
        continuing = match render_submission_response(&response, output, errors) {
            Ok(continuing) => continuing,
            Err(_) => return ExitCode::from(1),
        };
    }

    let closed = match request(
        server,
        request_id,
        "session.close",
        Some(&session),
        Value::Null,
        output,
        errors,
    ) {
        Ok(response) => response,
        Err(error) => return render_transport_error(&error, errors),
    };
    if closed.ok {
        ExitCode::SUCCESS
    } else {
        render_response_error(&closed, errors);
        ExitCode::from(1)
    }
}

fn submit_startup_source(
    server: &mut ServerProcess,
    request_id: u64,
    session: &str,
    source: &str,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> Result<(), ExitCode> {
    let response = request(
        server,
        request_id,
        "submit",
        Some(session),
        json!({ "source": source }),
        output,
        errors,
    )
    .map_err(|error| render_transport_error(&error, errors))?;
    let Ok(continuing) = render_submission_response(&response, output, errors) else {
        return Err(ExitCode::from(1));
    };
    if !response.ok {
        return Err(ExitCode::from(1));
    }
    if continuing {
        let _ = writeln!(errors, "slug-repl: startup source is incomplete");
        return Err(ExitCode::from(1));
    }
    Ok(())
}

fn render_transport_error(error: &io::Error, errors: &mut dyn Write) -> ExitCode {
    let _ = writeln!(errors, "slug-repl: slug-server protocol error: {error}");
    ExitCode::from(1)
}

trait SubmissionReader {
    fn read_submission(
        &mut self,
        output: &mut dyn Write,
        continuing: bool,
    ) -> io::Result<ReadSubmission>;
}

struct BufferedInput<'a> {
    input: &'a mut dyn BufRead,
}

impl SubmissionReader for BufferedInput<'_> {
    fn read_submission(
        &mut self,
        output: &mut dyn Write,
        continuing: bool,
    ) -> io::Result<ReadSubmission> {
        let mut line = String::new();
        let prompt = if continuing { ". " } else { "> " };
        write!(output, "{prompt}")?;
        output.flush()?;
        if self.input.read_line(&mut line)? == 0 {
            return Ok(ReadSubmission::Exit);
        }
        Ok(submission_from_line(line.trim_end_matches(['\n', '\r'])))
    }
}

struct TerminalInput {
    editor: DefaultEditor,
}

impl SubmissionReader for TerminalInput {
    fn read_submission(
        &mut self,
        _output: &mut dyn Write,
        continuing: bool,
    ) -> io::Result<ReadSubmission> {
        let prompt = if continuing { ". " } else { "> " };
        let line = match self.editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Eof) => return Ok(ReadSubmission::Exit),
            Err(error) => return Err(io::Error::other(error)),
        };
        let submission = submission_from_line(&line);
        if matches!(&submission, ReadSubmission::Source(source) if !source.is_empty()) {
            self.editor
                .add_history_entry(line)
                .map_err(io::Error::other)?;
        }
        Ok(submission)
    }
}

fn submission_from_line(line: &str) -> ReadSubmission {
    if line == ":quit" || line == ":exit" {
        return ReadSubmission::Exit;
    }
    ReadSubmission::Source(line.into())
}

fn request(
    server: &mut ServerProcess,
    id: u64,
    method: &str,
    session: Option<&str>,
    params: Value,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> io::Result<Response> {
    let request = Request {
        id,
        session: session.map(str::to_owned),
        method: method.into(),
        params: (!params.is_null()).then_some(params),
    };
    let (events, response) = server.client.request(&request)?;
    for event in events {
        render_event(&event, output, errors)?;
    }
    Ok(response)
}

struct ProtocolClient<R, W> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> ProtocolClient<R, W> {
    fn request(&mut self, request: &Request) -> io::Result<(Vec<Event>, Response)> {
        serde_json::to_writer(&mut self.writer, request).map_err(io::Error::other)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;

        let mut events = Vec::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "slug-server closed its protocol stream before responding",
                ));
            }
            let message = serde_json::from_str(&line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("slug-server emitted malformed protocol JSON: {error}"),
                )
            })?;
            match message {
                IncomingMessage::Event(event) => events.push(event),
                IncomingMessage::Response(response) if response.id == Some(request.id) => {
                    return Ok((events, *response));
                }
                IncomingMessage::Response(response) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "slug-server responded with id {:?}, expected {}",
                            response.id, request.id
                        ),
                    ));
                }
            }
        }
    }
}

struct ServerProcess {
    child: Child,
    client: ProtocolClient<BufReader<ChildStdout>, ChildStdin>,
}

impl ServerProcess {
    fn spawn() -> io::Result<Self> {
        let executable = env::var_os("SLUG_SERVER")
            .map(PathBuf::from)
            .map_or_else(sibling_server_path, Ok)?;
        let mut child = Command::new(&executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", executable.display()))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("slug-server stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("slug-server stdout was not piped"))?;
        Ok(Self {
            child,
            client: ProtocolClient {
                reader: BufReader::new(stdout),
                writer: stdin,
            },
        })
    }

    fn finish(self) -> io::Result<()> {
        let Self { mut child, client } = self;
        drop(client);
        let status = child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "slug-server exited with {status}"
            )))
        }
    }
}

fn sibling_server_path() -> io::Result<PathBuf> {
    let mut path = env::current_exe()?;
    path.set_file_name(format!("slug-server{}", env::consts::EXE_SUFFIX));
    Ok(path)
}

fn render_startup_response(response: &Response, errors: &mut dyn Write) -> Option<()> {
    response.ok.then_some(()).or_else(|| {
        render_response_error(response, errors);
        None
    })
}

fn render_submission_response(
    response: &Response,
    output: &mut dyn Write,
    errors: &mut dyn Write,
) -> io::Result<bool> {
    if !response.ok {
        render_response_error(response, errors);
        return Ok(false);
    }
    let Some(result) = &response.result else {
        return Ok(false);
    };
    if result.get("status").and_then(Value::as_str) == Some("incomplete") {
        return Ok(true);
    }
    match result.get("status").and_then(Value::as_str) {
        Some("completed") => {
            if let Some(value) = result.get("value").filter(|value| !value.is_null()) {
                writeln!(output, "{}", display_value(value))?;
            }
        }
        Some("stalled") => writeln!(output, "[stalled]")?,
        _ => {}
    }
    Ok(false)
}

fn render_event(event: &Event, output: &mut dyn Write, errors: &mut dyn Write) -> io::Result<()> {
    let stream: &mut dyn Write = if event.event == "stderr" {
        errors
    } else {
        output
    };
    match &event.data {
        Value::String(data) => write!(stream, "{data}"),
        data => writeln!(stream, "{}", display_value(data)),
    }
}

fn render_response_error(response: &Response, errors: &mut dyn Write) {
    let Some(diagnostic) = &response.error else {
        return;
    };
    let _ = render_diagnostic(diagnostic, errors);
}

fn render_diagnostic(diagnostic: &Diagnostic, errors: &mut dyn Write) -> io::Result<()> {
    render_diagnostic_indented(diagnostic, errors, "")
}

fn render_diagnostic_indented(
    diagnostic: &Diagnostic,
    errors: &mut dyn Write,
    indent: &str,
) -> io::Result<()> {
    let kind = diagnostic
        .kind
        .as_deref()
        .unwrap_or(diagnostic.code.as_str());
    let category = match diagnostic.category {
        DiagnosticCategory::Source => "source",
        DiagnosticCategory::Runtime => "runtime",
        DiagnosticCategory::Protocol => "protocol",
        DiagnosticCategory::Host => "host",
    };
    writeln!(
        errors,
        "{indent}{category} error ({kind}): {}",
        diagnostic.message
    )?;
    if let Some(location) = &diagnostic.location {
        writeln!(
            errors,
            "{indent}  --> {}:{}:{}",
            location.path, location.line, location.column
        )?;
    }
    if let Some(thrown) = &diagnostic.thrown {
        writeln!(errors, "{indent}  thrown: {}", thrown.display)?;
    }
    if let Some(native) = &diagnostic.native {
        write!(errors, "{indent}  native: {}", native.code)?;
        if let Some(data) = &native.data {
            write!(errors, " ({}: {})", data.kind, data.display)?;
        }
        writeln!(errors)?;
    }
    for frame in &diagnostic.frames {
        write!(errors, "{indent}  at {}", frame.function)?;
        if let Some(location) = &frame.location {
            write!(
                errors,
                " ({}:{}:{})",
                location.path, location.line, location.column
            )?;
        }
        writeln!(errors)?;
    }
    if let Some(cause) = &diagnostic.cause {
        writeln!(errors, "{indent}  caused by:")?;
        render_diagnostic_indented(cause, errors, &format!("{indent}    "))?;
    }
    Ok(())
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}
