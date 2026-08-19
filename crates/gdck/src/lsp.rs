//! A language server, so an editor gets what `gdck check` gives a terminal.
//!
//! Three things, which are the three `gdck` already knows how to do: report
//! problems as you type, format a document, and offer a rule's own fix as a
//! quick action. Nothing here decides anything the command line does not — the
//! same configuration is found the same way, and the same rules run.
//!
//! Synchronous, over stdio. `lsp-server` gives a channel of messages and this
//! reads them in a loop; there is no request `gdck` answers slowly enough to
//! need concurrency, since a whole 353-file corpus parses in about four
//! milliseconds and an editor only ever asks about one open file.
//!
//! ## What is deliberately not here
//!
//! Completion, go-to-definition and hover, which want a resolved view of the
//! project rather than one file's syntax, and which Godot's own language
//! server already provides. Running both is the expected arrangement — the
//! same one `ruff` and a type checker share in Python — so this offers what
//! Godot's does not rather than competing with it.

mod position;

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use gdck_config::Config;
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, InitializeParams, NumberOrString, OneOf, PositionEncodingKind,
    PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Uri, WorkspaceEdit,
    notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
        PublishDiagnostics,
    },
    request::{CodeActionRequest, Formatting, Request as _},
};

use position::{Encoding, Positions};

#[cfg(test)]
use std::str::FromStr as _;

/// The source of a rule's name in a diagnostic, shown by editors beside the
/// message.
const SOURCE: &str = "gdck";

/// Run the server until the client asks it to stop.
pub(crate) fn serve() -> Result<()> {
    let (connection, threads) = Connection::stdio();

    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let encoding = negotiate(&params);

    let capabilities = ServerCapabilities {
        position_encoding: Some(match encoding {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }),
        // Whole documents rather than incremental edits. A GDScript file is
        // small and the parser is fast enough that re-reading all of it costs
        // less than the bookkeeping incremental sync would need, and cannot
        // drift out of step with the client's copy.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..CodeActionOptions::default()
        })),
        ..ServerCapabilities::default()
    };
    connection.initialize_finish(
        id,
        serde_json::json!({
            "capabilities": capabilities,
            "serverInfo": { "name": "gdck", "version": env!("CARGO_PKG_VERSION") },
        }),
    )?;

    let mut server = Server {
        encoding,
        documents: HashMap::new(),
    };
    server.run(&connection)?;

    // Before joining, and not incidentally: the writer thread runs until its
    // channel closes, and the channel closes when the connection is dropped.
    // Joining first waits for a thread that is waiting for this.
    drop(connection);
    threads.join()?;
    Ok(())
}

/// Which encoding to count positions in.
///
/// The protocol's default is UTF-16 and a client that says nothing gets it.
/// UTF-8 is taken when offered, since `gdck`'s offsets are already bytes and
/// converting them costs nothing.
fn negotiate(params: &InitializeParams) -> Encoding {
    let offered = params
        .capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());
    match offered {
        Some(kinds) if kinds.contains(&PositionEncodingKind::UTF8) => Encoding::Utf8,
        _ => Encoding::Utf16,
    }
}

struct Server {
    encoding: Encoding,
    /// The client's copy of every open file. The disk is not read while a file
    /// is open: what the editor shows is what gets checked, unsaved and all.
    documents: HashMap<Uri, String>,
}

impl Server {
    fn run(&mut self, connection: &Connection) -> Result<()> {
        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    if connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    self.request(connection, request)?;
                }
                Message::Notification(notification) => {
                    self.notification(connection, notification)?;
                }
                // Responses to requests this server made, and it makes none.
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn request(&mut self, connection: &Connection, request: Request) -> Result<()> {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            Formatting::METHOD => {
                let params: DocumentFormattingParams = serde_json::from_value(request.params)?;
                serde_json::to_value(self.formatting(&params.text_document.uri))?
            }
            CodeActionRequest::METHOD => {
                let params: CodeActionParams = serde_json::from_value(request.params)?;
                serde_json::to_value(actions(&params))?
            }
            // Anything else is answered with null rather than an error: the
            // capabilities said what is on offer, and a client asking anyway
            // wants an answer it can ignore, not a failure in its log.
            _ => serde_json::Value::Null,
        };
        connection
            .sender
            .send(Message::Response(Response::new_ok(id, result)))?;
        Ok(())
    }

    fn notification(&mut self, connection: &Connection, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                let uri = params.text_document.uri;
                self.documents
                    .insert(uri.clone(), params.text_document.text);
                self.publish(connection, &uri)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                // Full sync, so the last change carries the whole document.
                if let Some(change) = params.content_changes.into_iter().next_back() {
                    let uri = params.text_document.uri;
                    self.documents.insert(uri.clone(), change.text);
                    self.publish(connection, &uri)?;
                }
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.documents.remove(&params.text_document.uri);
                // An empty list clears what was shown; without it the last
                // diagnostics for the file stay in the editor's problem list
                // after it is closed.
                clear(connection, &params.text_document.uri)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn publish(&self, connection: &Connection, uri: &Uri) -> Result<()> {
        let Some(text) = self.documents.get(uri) else {
            return Ok(());
        };
        let config = config_for(uri);
        let diagnostics = diagnose(text, &config, uri, self.encoding);
        connection
            .sender
            .send(Message::Notification(Notification::new(
                PublishDiagnostics::METHOD.to_string(),
                PublishDiagnosticsParams {
                    uri: uri.clone(),
                    diagnostics,
                    version: None,
                },
            )))?;
        Ok(())
    }

    /// The formatted document, as one edit replacing all of it.
    ///
    /// Nothing when the file does not parse or a safety check refuses it,
    /// which is the same promise the command line makes: a file the formatter
    /// cannot vouch for is left exactly as it is.
    fn formatting(&self, uri: &Uri) -> Option<Vec<TextEdit>> {
        let text = self.documents.get(uri)?;
        let config = config_for(uri);
        let tree = gdck_syntax::parse(text);
        let formatted = gdck_format::format(&tree, &config.format).ok()?;
        if formatted == *text {
            return Some(Vec::new());
        }
        Some(vec![TextEdit {
            range: Positions::new(text, self.encoding).whole(),
            new_text: formatted,
        }])
    }
}

/// The file a `file://` URI names, if it names one.
///
/// `lsp-types` stopped carrying `url::Url` and its `to_file_path`, so this
/// does the two things that conversion has to get right: percent-decoding, or
/// a project under a path with a space in it finds no configuration; and the
/// leading slash Windows drive letters arrive with, as `file:///C:/game`.
///
/// Anything that is not a `file://` URI — an untitled buffer, a document
/// inside an archive — gives `None`, and the caller falls back to defaults.
fn file_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    // Skip an authority, which for a local file is empty or `localhost`.
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(at) if &rest[..at] == "localhost" => &rest[at..],
        _ => return None,
    };

    let mut decoded = Vec::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let text = String::from_utf8(decoded).ok()?;

    // `file:///C:/game` is a Windows path wearing a leading slash.
    let trimmed = text
        .strip_prefix('/')
        .filter(|rest| {
            let mut chars = rest.chars();
            matches!(
                (chars.next(), chars.next()),
                (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
            )
        })
        .unwrap_or(&text);
    Some(PathBuf::from(trimmed))
}

/// The settings a file would be checked with from the command line.
///
/// Found from the file's own path, so a repository holding more than one Godot
/// project gets each one's settings rather than whichever the editor happened
/// to open at. Falls back to the style guide's defaults when the path is not
/// one on disk or nothing can be read, since an editor is a worse place than a
/// terminal to be told about a malformed configuration file.
fn config_for(uri: &Uri) -> Config {
    file_path(uri)
        .and_then(|path| {
            let start = path.parent()?.to_path_buf();
            gdck_config::resolve(&start).ok()
        })
        .map_or_else(Config::default, |loaded| loaded.config)
}

/// Everything wrong with a document, as the protocol's diagnostics.
///
/// Syntax errors and lint findings both, because an editor shows one list.
/// A fixable rule carries its edits in `data`, which is what lets a code
/// action offer the fix without the server having to work out which diagnostic
/// a cursor is sitting in.
fn diagnose(text: &str, config: &Config, uri: &Uri, encoding: Encoding) -> Vec<Diagnostic> {
    let positions = Positions::new(text, encoding);
    let tree = gdck_syntax::parse(text);
    let mut diagnostics = Vec::new();

    for error in tree.errors() {
        diagnostics.push(Diagnostic {
            range: positions.range(error.range()),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("syntax-error".to_string())),
            source: Some(SOURCE.to_string()),
            message: error.message().to_string(),
            ..Diagnostic::default()
        });
    }

    let name = file_path(uri).and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    });
    for found in gdck_lint::lint_file(&tree, &config.lint, name.as_deref()) {
        let edits: Vec<TextEdit> = found
            .fix
            .as_ref()
            .map(|fix| {
                fix.edits
                    .iter()
                    .map(|edit| TextEdit {
                        range: positions.range(edit.range),
                        new_text: edit.text.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        diagnostics.push(Diagnostic {
            range: positions.range(found.range),
            severity: Some(match found.severity {
                gdck_lint::Severity::Error => DiagnosticSeverity::ERROR,
                gdck_lint::Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            code: Some(NumberOrString::String(found.rule.to_string())),
            source: Some(SOURCE.to_string()),
            message: found.message,
            data: (!edits.is_empty())
                .then(|| serde_json::to_value(&edits).ok())
                .flatten(),
            ..Diagnostic::default()
        });
    }
    diagnostics
}

/// A quick fix for each diagnostic under the cursor that carries one.
///
/// The edits were converted when the diagnostic was published and travel back
/// in `data`, so this never has to turn the client's positions into offsets —
/// the one direction this server does not implement, and one fewer place for
/// an encoding to be got wrong.
fn actions(params: &CodeActionParams) -> CodeActionResponse {
    let mut actions = Vec::new();
    for diagnostic in &params.context.diagnostics {
        if diagnostic.source.as_deref() != Some(SOURCE) {
            continue;
        }
        let Some(data) = &diagnostic.data else {
            continue;
        };
        let Ok(edits) = serde_json::from_value::<Vec<TextEdit>>(data.clone()) else {
            continue;
        };
        let rule = match &diagnostic.code {
            Some(NumberOrString::String(rule)) => rule.clone(),
            _ => SOURCE.to_string(),
        };
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Fix `{rule}`"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diagnostic.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(
                    [(params.text_document.uri.clone(), edits)]
                        .into_iter()
                        .collect(),
                ),
                ..WorkspaceEdit::default()
            }),
            ..CodeAction::default()
        }));
    }
    actions
}

fn clear(connection: &Connection, uri: &Uri) -> Result<()> {
    connection
        .sender
        .send(Message::Notification(Notification::new(
            PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams {
                uri: uri.clone(),
                diagnostics: Vec::new(),
                version: None,
            },
        )))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Uri {
        Uri::from_str("file:///project/a.gd").expect("a valid uri")
    }

    #[test]
    fn utf8_is_taken_only_when_the_client_offers_it() {
        // A client that says nothing gets the protocol's default, which is the
        // whole reason the conversion exists.
        let silent = InitializeParams::default();
        assert_eq!(negotiate(&silent), Encoding::Utf16);

        let mut offering = InitializeParams::default();
        offering.capabilities.general = Some(lsp_types::GeneralClientCapabilities {
            position_encodings: Some(vec![PositionEncodingKind::UTF8]),
            ..lsp_types::GeneralClientCapabilities::default()
        });
        assert_eq!(negotiate(&offering), Encoding::Utf8);

        let mut utf16_only = InitializeParams::default();
        utf16_only.capabilities.general = Some(lsp_types::GeneralClientCapabilities {
            position_encodings: Some(vec![PositionEncodingKind::UTF16]),
            ..lsp_types::GeneralClientCapabilities::default()
        });
        assert_eq!(negotiate(&utf16_only), Encoding::Utf16);
    }

    #[test]
    fn syntax_errors_and_lint_findings_arrive_in_one_list() {
        // An editor shows one problem list, so both kinds have to be in it.
        let text = "extends Node\n\nfunc f( ->\n\t)))\n";
        let found = diagnose(text, &Config::default(), &url(), Encoding::Utf16);
        assert!(
            found
                .iter()
                .any(|d| d.code == Some(NumberOrString::String("syntax-error".to_string()))),
            "{found:#?}"
        );
        assert!(
            found.iter().all(|d| d.source.as_deref() == Some("gdck")),
            "every diagnostic says where it came from"
        );
    }

    #[test]
    fn a_fixable_rule_carries_its_edits_for_a_code_action() {
        let text = "extends Node\n\nvar a = 'x'\n";
        let found = diagnose(text, &Config::default(), &url(), Encoding::Utf16);
        let quote = found
            .iter()
            .find(|d| d.code == Some(NumberOrString::String("quote-style".to_string())))
            .expect("quote-style should report");
        let edits: Vec<TextEdit> =
            serde_json::from_value(quote.data.clone().expect("a fix travels in data"))
                .expect("the edits should read back");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "\"x\"");

        // And that is enough to build the action, without the client's
        // positions ever being converted back to offsets.
        let params = CodeActionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: url() },
            range: quote.range,
            context: lsp_types::CodeActionContext {
                diagnostics: vec![quote.clone()],
                ..lsp_types::CodeActionContext::default()
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        };
        let offered = actions(&params);
        assert_eq!(offered.len(), 1, "{offered:#?}");
    }

    #[test]
    fn a_diagnostic_from_another_server_is_not_ours_to_fix() {
        // Godot's own language server is expected to be running alongside this
        // one, and its diagnostics arrive in the same request.
        let params = CodeActionParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: url() },
            range: lsp_types::Range::default(),
            context: lsp_types::CodeActionContext {
                diagnostics: vec![Diagnostic {
                    source: Some("godot".to_string()),
                    data: Some(serde_json::json!([])),
                    ..Diagnostic::default()
                }],
                ..lsp_types::CodeActionContext::default()
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        };
        assert!(actions(&params).is_empty());
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        let text = "extends Node\n\n\nfunc _ready() -> void:\n\tprint(\"hi\")\n";
        let found = diagnose(text, &Config::default(), &url(), Encoding::Utf16);
        assert!(found.is_empty(), "{found:#?}");
    }

    #[test]
    fn a_uri_becomes_the_path_it_names() {
        assert_eq!(
            file_path(&Uri::from_str("file:///project/a.gd").unwrap()),
            Some(PathBuf::from("/project/a.gd"))
        );
        // Percent-decoded, or a project under a path with a space in it finds
        // no configuration at all.
        assert_eq!(
            file_path(&Uri::from_str("file:///My%20Games/shrine/a.gd").unwrap()),
            Some(PathBuf::from("/My Games/shrine/a.gd"))
        );
        // Windows arrives with a slash in front of the drive letter.
        assert_eq!(
            file_path(&Uri::from_str("file:///C:/game/a.gd").unwrap()),
            Some(PathBuf::from("C:/game/a.gd"))
        );
        // A leading slash that is not a drive letter is part of the path.
        assert_eq!(
            file_path(&Uri::from_str("file:///c/game/a.gd").unwrap()),
            Some(PathBuf::from("/c/game/a.gd"))
        );
    }

    #[test]
    fn something_that_is_not_a_file_has_no_path() {
        // An unsaved buffer, which an editor is entitled to open and ask about.
        assert_eq!(
            file_path(&Uri::from_str("untitled:Untitled-1").unwrap()),
            None
        );
        assert_eq!(
            file_path(&Uri::from_str("https://example.com/a.gd").unwrap()),
            None
        );
    }
}
