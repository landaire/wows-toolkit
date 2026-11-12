//! The system dialog on Windows and macOS, through rfd.

use futures::future::Either;

use super::Answer;
use super::Asked;
use super::Request;

pub(super) fn ask(request: Request) -> impl Future<Output = Answer> + use<> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = &request.title {
        dialog = dialog.set_title(title);
    }
    if let Some(filter) = request.filter {
        dialog = dialog.add_filter(filter.label, filter.extensions);
    }
    if let Some(start) = &request.start {
        dialog = dialog.set_directory(start);
    }

    // Each picker is its own future type; the dialog opens here, when it is asked for.
    let picked = match request.asked {
        Asked::File => Either::Left(dialog.pick_file()),
        Asked::Folder => Either::Right(Either::Left(dialog.pick_folder())),
        Asked::Save { file_name } => Either::Right(Either::Right(dialog.set_file_name(file_name).save_file())),
    };

    // These dialogs report no failure: `None` is always a dismissal.
    async move { Ok(picked.await.map(|handle| handle.path().to_path_buf())) }
}
