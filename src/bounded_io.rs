//! Limites medidos sobre bytes descompactados, independentemente do cabecalho ZIP.
use crate::error::{AppError, AppResult};
use std::io::{Read, Write};

/// Copia ate o limite e detecta um byte adicional sem grava-lo no destino.
pub fn copy_limited(reader: &mut impl Read, writer: &mut impl Write, limit: u64) -> AppResult<u64> {
    let copied = std::io::copy(&mut reader.take(limit), writer)?;
    let mut extra = [0_u8; 1];
    if reader.read(&mut extra)? != 0 {
        return Err(AppError::BadRequest(
            "conteudo descompactado excede o limite".into(),
        ));
    }
    Ok(copied)
}

/// Le uma entrada em memoria respeitando o limite efetivo.
pub fn read_limited(reader: &mut impl Read, limit: u64) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    copy_limited(reader, &mut bytes, limit)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "../tests/rust/bounded-io.rs"]
mod tests;
