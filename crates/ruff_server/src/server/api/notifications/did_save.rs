use super::{DidChangeWatchedFiles, SyncNotificationHandler};
use crate::server::Result;
use crate::session::{Client, Session};
use lsp_types::{
    self as types, DidChangeWatchedFilesParams, DidSaveTextDocumentNotification, FileChangeType,
    FileEvent,
};

pub(crate) struct DidSave;

impl super::NotificationHandler for DidSave {
    type NotificationType = DidSaveTextDocumentNotification;
}

impl SyncNotificationHandler for DidSave {
    fn run(
        session: &mut Session,
        client: &Client,
        params: types::DidSaveTextDocumentParams,
    ) -> Result<()> {
        let uri = params.text_document.uri;
        if !session.reloads_python_rules_on_save(&uri) {
            return Ok(());
        }
        DidChangeWatchedFiles::run(
            session,
            client,
            DidChangeWatchedFilesParams {
                changes: vec![FileEvent {
                    uri,
                    kind: FileChangeType::Changed,
                }],
            },
        )
    }
}
