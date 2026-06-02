//! Private parquet-metadata touchpoint for the scalar reader (architecture §1).
//!
//! This module is the crate's single entry into the pure-Rust `parquet`/`arrow`
//! stack (the R1 decision; see `architecture.md` Amendments log, R1-parquet). It
//! opens a parquet **byte source** and recovers its **metadata only** — the arrow
//! [`SchemaRef`] (column names + types) and the **row-group count** — never any
//! gridded chunk or data-column values. This is the cheap discovery read that
//! `validate`/`describe` are built on (architecture §1: read metadata, not chunks).
//!
//! MS3-S1 lands this helper plus its tests so the crate **exercises** the pinned
//! dependency end-to-end before any reader module is built. S3 layers the scalar
//! field catalog, `basin_id`/`time` descriptors, and the row-group-statistics time
//! extent on top of this same metadata path.
//!
//! Scope: the helper is `pub(crate)` — it is an internal touchpoint, not public
//! API. It reads only the footer/schema metadata; it decodes no row group.

use std::sync::Arc;

use arrow::datatypes::Schema;
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::metadata::ParquetMetaData;
use tracing::{debug, instrument};

use crate::error::CoreError;

/// The metadata recovered from a parquet byte source, with no chunk decoded.
///
/// Holds exactly the facts the scalar reader needs from the footer: the arrow
/// [`Schema`] (column names + types) and the **row-group count**. It is
/// **inert/agnostic** (spec §1): it carries the physical metadata and nothing
/// derived — no transform, role, semantic type, or provenance.
#[derive(Debug, Clone)]
pub(crate) struct ParquetMeta {
    /// The arrow schema decoded from the parquet footer (column names + types).
    schema: Arc<Schema>,
    /// The full parquet file metadata (footer): row-group metadata, statistics,
    /// and per-column descriptors. S3 reads row-group `time` statistics from here.
    file_metadata: Arc<ParquetMetaData>,
}

impl ParquetMeta {
    /// Borrows the arrow schema decoded from the footer.
    pub(crate) fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Returns the number of row groups recorded in the footer.
    pub(crate) fn num_row_groups(&self) -> usize {
        self.file_metadata.num_row_groups()
    }

    /// Borrows the full parquet file metadata (footer).
    ///
    /// Used by S3 to read row-group `time` statistics for the per-basin time
    /// extent; it exposes no chunk data.
    pub(crate) fn file_metadata(&self) -> &ParquetMetaData {
        &self.file_metadata
    }
}

/// Opens a parquet byte source and recovers its metadata (schema + row groups).
///
/// `artifact` is a name used only for diagnostics and error messages (a path, or a
/// label such as `"<in-memory>"` for an in-test buffer); it is never interpreted.
/// `bytes` is the raw parquet file content — the helper reads the footer and arrow
/// schema from it without decoding any row group (architecture §1).
///
/// # Errors
///
/// | Condition | Error |
/// |---|---|
/// | `bytes` is not a valid parquet file, or its footer/metadata fails to decode | [`CoreError::ParquetRead`] (with `artifact` echoed and `detail` from the reader) |
#[instrument(skip(bytes), fields(artifact = artifact, len = bytes.len()))]
pub(crate) fn read_parquet_meta(artifact: &str, bytes: &[u8]) -> Result<ParquetMeta, CoreError> {
    let source = Bytes::copy_from_slice(bytes);

    let builder =
        ParquetRecordBatchReaderBuilder::try_new(source).map_err(|e| CoreError::ParquetRead {
            artifact: artifact.to_string(),
            detail: e.to_string(),
        })?;

    let schema = Arc::clone(builder.schema());
    let file_metadata = Arc::clone(builder.metadata());

    debug!(
        columns = schema.fields().len(),
        row_groups = file_metadata.num_row_groups(),
        "read parquet metadata"
    );

    Ok(ParquetMeta {
        schema,
        file_metadata,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::Int32Array;
    use arrow::datatypes::{DataType, Field as ArrowField, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;

    use crate::error::CoreError;
    use crate::parquet_meta::read_parquet_meta;

    /// Writes a tiny one-column `int32` parquet table to an in-memory `Vec<u8>`.
    ///
    /// Dev-only path: this exercises the pinned `parquet` writer so the metadata
    /// helper can be tested end-to-end without a committed fixture.
    fn tiny_int32_parquet(column: &str) -> Vec<u8> {
        let schema = Arc::new(Schema::new(vec![ArrowField::new(
            column,
            DataType::Int32,
            false,
        )]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(Int32Array::from(vec![1, 2, 3])) as _],
        )
        .expect("building a 1-column int32 batch must succeed");

        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut writer = ArrowWriter::try_new(&mut buffer, schema, None)
                .expect("constructing the arrow parquet writer must succeed");
            writer.write(&batch).expect("writing the batch must succeed");
            writer.close().expect("closing the writer must succeed");
        }
        buffer
    }

    #[test]
    fn recovers_schema_and_row_group_count_from_in_memory_buffer() {
        let bytes = tiny_int32_parquet("value");

        let meta = read_parquet_meta("<in-memory>", &bytes)
            .expect("a well-formed parquet buffer must yield metadata");

        // Schema recovery: exactly one column, named `value`, typed `Int32`.
        let fields = meta.schema().fields();
        assert_eq!(fields.len(), 1, "the tiny table has exactly one column");
        assert_eq!(fields[0].name(), "value");
        assert_eq!(fields[0].data_type(), &DataType::Int32);

        // Row-group recovery: a single small write lands in one row group.
        assert_eq!(meta.num_row_groups(), 1);
        assert_eq!(meta.file_metadata().num_row_groups(), 1);
    }

    #[test]
    fn non_parquet_bytes_return_parquet_read_without_panicking() {
        // Arbitrary non-parquet bytes (no parquet magic footer) must surface a
        // typed error, never a panic.
        let garbage = b"this is definitely not a parquet file";

        match read_parquet_meta("garbage.parquet", garbage) {
            Err(CoreError::ParquetRead { artifact, detail }) => {
                assert_eq!(artifact, "garbage.parquet", "the artifact name is echoed");
                assert!(!detail.is_empty(), "the reader detail must be populated");
            }
            other => panic!("expected CoreError::ParquetRead, got {other:?}"),
        }
    }

    #[test]
    fn empty_bytes_return_parquet_read_without_panicking() {
        match read_parquet_meta("empty.parquet", &[]) {
            Err(CoreError::ParquetRead { artifact, .. }) => {
                assert_eq!(artifact, "empty.parquet");
            }
            other => panic!("expected CoreError::ParquetRead for empty input, got {other:?}"),
        }
    }
}
