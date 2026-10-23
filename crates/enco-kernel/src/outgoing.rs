use crate::{Kernel, KernelError, Store, StoreError};
use enco_core::{FileData, Outgoing, OutgoingFile};

impl Kernel {
    /// Resolve the recorded attachment before a channel starts its delivery.
    pub async fn outgoing_file(&self, file: &OutgoingFile) -> Result<FileData, KernelError> {
        Ok(FileData {
            name: file.name.clone(),
            data: self.deps.store.get_blob(&file.content).await?,
        })
    }
}

pub(crate) async fn store(
    store: &dyn Store,
    outgoing: Outgoing<FileData>,
) -> Result<Outgoing, StoreError> {
    let file = match outgoing.file {
        Some(file) => Some(OutgoingFile {
            size: file.data.len() as u64,
            content: store.put_blob(&file.data).await?,
            name: file.name,
        }),
        None => None,
    };
    Ok(Outgoing {
        text: outgoing.text,
        file,
    })
}
