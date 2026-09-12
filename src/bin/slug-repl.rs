use std::{
    io::{self, BufRead, Write},
    process::ExitCode,
};

use serde_json::{Value, json};
use slug_vm::interactive::{
    Diagnostic, DiagnosticCategory, Event, PROTOCOL_VERSION, Response, Server,
};
use slug_vm::source_is_incomplete;

fn main() -> ExitCode {
    run(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

enum ReadSubmission {
    Empty,
    Exit,
    Source { source: String, input_closed: bool },
}

fn run(input: &mut dyn BufRead, output: &mut dyn Write, errors: &mut dyn Write) -> ExitCode {
    let mut server = Server::default();
    let mut request_id = 1;
    let initialized = request(
        &mut server,
        request_id,
        "initialize",
        None,
        json!({
            "protocol": PROTOCOL_VERSION,
        }),
    );
    request_id += 1;
    let Some(()) = render_startup_response(&initialized, errors) else {
        return ExitCode::from(1);
    };
    let opened = request(&mut server, request_id, "session.open", None, Value::Null);
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

    if writeln!(output, "Slug REPL\n").is_err() {
        return ExitCode::from(1);
    }
    loop {
        let (source, input_closed) = match read_submission(input, output, &session) {
            Ok(ReadSubmission::Empty) => continue,
            Ok(ReadSubmission::Exit) => break,
            Ok(ReadSubmission::Source {
                source,
                input_closed,
            }) => (source, input_closed),
            Err(error) => {
                let _ = writeln!(errors, "input error: {error}");
                return ExitCode::from(1);
            }
        };
        let response = request(
            &mut server,
            request_id,
            "submit",
            Some(&session),
            json!({ "source": source }),
        );
        request_id += 1;
        for event in server.take_events() {
            if render_event(&event, output, errors).is_err() {
                return ExitCode::from(1);
            }
        }
        if render_submission_response(&response, output, errors).is_err() {
            return ExitCode::from(1);
        }
        if input_closed {
            break;
        }
    }

    let closed = request(
        &mut server,
        request_id,
        "session.close",
        Some(&session),
        Value::Null,
    );
    if closed.ok {
        ExitCode::SUCCESS
    } else {
        render_response_error(&closed, errors);
        ExitCode::from(1)
    }
}

fn read_submission(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    session: &str,
) -> io::Result<ReadSubmission> {
    let mut line = String::new();
    let mut source = String::new();
    loop {
        let prompt = if source.is_empty() { "> " } else { ". " };
        write!(output, "{prompt}")?;
        output.flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(if source.is_empty() {
                ReadSubmission::Exit
            } else {
                ReadSubmission::Source {
                    source,
                    input_closed: true,
                }
            });
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if source.is_empty() && (line == ":quit" || line == ":exit") {
            return Ok(ReadSubmission::Exit);
        }
        if source.is_empty() && line.is_empty() {
            return Ok(ReadSubmission::Empty);
        }
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(line);
        if !source_is_incomplete(&format!("<interactive:{session}>"), &source) {
            return Ok(ReadSubmission::Source {
                source,
                input_closed: false,
            });
        }
    }
}

fn request(
    server: &mut Server,
    id: u64,
    method: &str,
    session: Option<&str>,
    params: Value,
) -> Response {
    let mut request = json!({ "id": id, "method": method });
    if let Some(session) = session {
        request["session"] = json!(session);
    }
    if !params.is_null() {
        request["params"] = params;
    }
    server.handle_line(&request.to_string())
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
) -> io::Result<()> {
    if !response.ok {
        render_response_error(response, errors);
        return Ok(());
    }
    let Some(result) = &response.result else {
        return Ok(());
    };
    if let Some(value) = result.get("value") {
        if !value.is_null() {
            writeln!(output, "{}", display_value(value))?;
        }
    } else if let Some(state) = result.get("state").and_then(Value::as_str) {
        writeln!(output, "[{state}]")?;
    }
    Ok(())
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
    writeln!(errors, "{category} error ({kind}): {}", diagnostic.message)?;
    if let Some(location) = &diagnostic.location {
        writeln!(
            errors,
            "  --> {}:{}:{}",
            location.path, location.line, location.column
        )?;
    }
    Ok(())
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}
