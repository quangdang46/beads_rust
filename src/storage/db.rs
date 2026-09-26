//! Shape-compat adapter between the frankensqlite value/row API and `rusqlite`'s.
//!
//! # THIS FILE IS DELETED BEFORE THE MIGRATION MERGES
//!
//! It exists only to make the port mechanical. `main` must never carry it.
//!
//! **If this file still exists when Phase 8 opens, Phase 8 does not merge.** That is the same
//! deletion gate the plan already sets on the `DatabaseLegacy` error variant and on nothing
//! else. It is not a compatibility shim in the sense `AGENTS.md` forbids: it wraps nothing
//! deprecated, and it provides no backwards compatibility for any caller. It absorbs the
//! shape difference between two engines during a single atomic change, and it is deleted
//! before that change lands on `main`.
//!
//! # Why it exists
//!
//! The port touches ~2,400 call sites. Three of the dominant patterns account for most of
//! them, and all three are *shape* problems rather than logic problems:
//!
//! | pattern | sites | problem it causes |
//! |---|---|---|
//! | `SqliteValue::from(x)` | 674 in `src/` | the type is foreign and disappears in Phase 8 |
//! | `row.get(i).and_then(as_text)` | ~250 | frankensqlite returns owned rows; rusqlite borrows |
//! | `conn.execute_with_params(..)` | 138 | the parameter container differs |
//!
//! Left alone, each of those is a hand-written rewrite with a type annotation and an error
//! arm, times hundreds, and the chance of one of them being wrong is the chance of a silent
//! data bug. Routing them through one adapter turns them into type-path churn, which is
//! reviewable by eye.
//!
//! # What it deliberately does NOT abstract
//!
//! - **`Row`.** `doctor.rs:23` and `federation.rs:90` take `&Row` / `&fsqlite::Row` as a named
//!   type. rusqlite's `Row` borrows from its `Statement` and is not `'static`, so those
//!   signatures need owned-row collection rather than a type rename. `query_rows` is the
//!   mechanism; it is not a `Row` replacement and does not pretend to be one.
//! - **Storage classes.** SQLite is dynamically typed, and a typed `get` compiles even when a
//!   column's stored class shifts. The adapter cannot detect that; the byte-identical golden
//!   comparison in Phase 6 is what detects it. Do not add a conversion that "helpfully"
//! coerces, because a coercion here is exactly the class of bug the golden exists to catch.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, Value, ValueRef};
use rusqlite::fallible_streaming_iterator::FallibleStreamingIterator as _;
use rusqlite::{Connection, Error, Row, Statement};

/// A bindable value, shaped so that the `SqliteValue::from(x)` call sites keep working
/// verbatim once the type is renamed to `SqlValue`.
///
/// The `From` impls deliberately mirror `fsqlite_types::SqliteValue`'s set exactly. A missing
/// one turns a type-path rename into 674 hand edits, which is the whole thing this file
/// exists to avoid, so adding a variant here is a mechanical, non-breaking change.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlValue(Value);

impl SqlValue {
    /// Wrap a raw `rusqlite` value.
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// SQL `NULL`.
    ///
    /// The direct analogue of `fsqlite_types::SqliteValue::Null`, which appears at ~71 sites
    /// as the nullable-column idiom. It cannot be a `From` impl, because there is no
    /// distinguishing type to convert *from*.
    #[must_use]
    pub fn null() -> Self {
        Self(Value::Null)
    }

    /// Borrow the underlying value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.0
    }

    /// Consume into the underlying value.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.0
    }

    /// Text, or `None` if this value is not text.
    ///
    /// Note this returns `None` for a non-text value, exactly as
    /// `fsqlite_types::SqliteValue::as_text` did. It does NOT coerce integers to strings, and
    /// it must not: doing so would silently mask a column whose storage class changed, which
    /// is precisely the failure the golden snapshots exist to catch.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match &self.0 {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Integer, or `None` if this value is not an integer.
    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match &self.0 {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Float, or `None` if this value is not a float.
    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match &self.0 {
            Value::Real(f) => Some(*f),
            _ => None,
        }
    }

    /// Blob, or `None` if this value is not a blob.
    #[must_use]
    pub fn as_blob(&self) -> Option<&[u8]> {
        match &self.0 {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// True for `NULL`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self.0, Value::Null)
    }
}

// Mirrors of `fsqlite_types::SqliteValue`'s `From` set, in the same order.
macro_rules! impl_from {
    ($($from:ty => $conv:expr),* $(,)?) => {
        $(
            impl From<$from> for SqlValue {
                #[inline]
                fn from(v: $from) -> Self {
                    // The `From` is on `Value`, so the conversion is a single deref and the
                    // body stays trivial.
                    let inner: Value = $conv(v);
                    Self(inner)
                }
            }
        )*
    };
}

impl_from! {
    i64 => Value::from,
    i32 => |v: i32| Value::from(i64::from(v)),
    f64 => Value::from,
    String => Value::from,
    &str => |v: &str| Value::from(v.to_owned()),
    std::sync::Arc<str> => |v: std::sync::Arc<str>| Value::from(v.to_string()),
    Vec<u8> => |v: Vec<u8>| Value::from(v),
    &[u8] => |v: &[u8]| Value::from(v.to_vec()),
    std::sync::Arc<[u8]> => |v: std::sync::Arc<[u8]>| Value::from(v.to_vec()),
}

impl ToSql for SqlValue {
    /// rusqlite 0.40 returns a `ToSqlOutput`, not a `Value`. An owned value is correct here
    /// because `SqlValue` always owns its data and hands rusqlite a borrow of it.
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(self.0.clone()))
    }
}

/// Borrow a row's values as owned `SqlValue`s.
///
/// Mirrors `fsqlite`'s `Row::values() -> &[SqliteValue]`, which returns an owned slice.
/// rusqlite has no equivalent, so this is the adapter for the ~9 sites in `doctor.rs` that
/// depend on that owned-slice shape.
pub fn row_values(row: &Row<'_>) -> rusqlite::Result<Vec<SqlValue>> {
    column_values(row, 0, row.as_ref().column_count())
}

/// Read `count` columns starting at `offset` as owned `SqlValue`s.
pub fn column_values(
    row: &Row<'_>,
    offset: usize,
    count: usize,
) -> rusqlite::Result<Vec<SqlValue>> {
    let total = row.as_ref().column_count();
    let end = offset.saturating_add(count).min(total);
    (offset..end)
        .map(|i| value_ref_to_value(row.get_ref(i)?).map(SqlValue::new))
        .collect()
}

/// Copy a borrowed `ValueRef` into an owned `Value`.
///
/// `ValueRef` borrows from the row, and the `Row::values()` call sites want ownership, so the
/// borrow is discharged here rather than at each site.
///
/// **This decodes UTF-8 and can fail.** In rusqlite 0.40 `ValueRef::Text` is a `&[u8]`, and
/// text is validated as UTF-8 only when a `str` is requested. Converting to an owned `Value`
/// therefore has to decode now, and a column holding bytes that are not valid UTF-8 must
/// surface as an error. Lossy decoding here would be silent data corruption in exactly the
/// place the Phase 6 golden comparison is supposed to be able to detect a shift.
pub fn value_ref_to_value(value: ValueRef<'_>) -> rusqlite::Result<Value> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Integer(i),
        ValueRef::Real(f) => Value::Real(f),
        ValueRef::Text(t) => Value::Text(std::str::from_utf8(t)?.to_owned()),
        ValueRef::Blob(b) => Value::Blob(b.to_owned()),
    })
}

/// Read one column as an owned `SqlValue`.
///
/// Replaces the `row.get(idx) -> Option<&SqliteValue>` shape. Note the loss: frankensqlite
/// returned `Option`, and this returns `Result`. A column that is absent, and a column whose
/// value failed to convert, are now distinguishable only by inspecting the error, so callers
/// that genuinely want the old lenient behaviour should use [`column_values`] and match.
pub fn row_value(row: &Row<'_>, idx: usize) -> rusqlite::Result<SqlValue> {
    value_ref_to_value(row.get_ref(idx)?).map(SqlValue::new)
}

/// Collect every row of a statement eagerly, as owned column values.
///
/// **Mandatory, not an optimisation, and it cannot return `Vec<Row>`.** frankensqlite returned
/// an owned `Vec<Row>`. In rusqlite 0.40 a `Row` is a handle onto its `Statement`: it has no
/// `Clone`, and the streaming iterator's `next` yields `&Row`, so there is no way to lift rows
/// out of a `Rows` that has been dropped. Collecting the *values* is the only owned shape
/// available, and it is the one the call sites actually want -- they read columns, not
/// statement positions.
///
/// The consequence is that the two `&Row` / `&fsqlite::Row` signatures in the tree
/// (`doctor.rs`, `federation.rs`) become `&[SqlValue]`. That is a real signature change and is
/// the correct one; the plan flags it as a Phase 3 hazard, and this is why.
pub fn query_rows(stmt: &mut Statement<'_>) -> rusqlite::Result<Vec<Vec<SqlValue>>> {
    let total = stmt.column_count();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(column_values(row, 0, total)?);
    }
    Ok(out)
}

/// Read a single row as owned column values, or `None` if the query matched nothing.
pub fn query_row_values(stmt: &mut Statement<'_>) -> rusqlite::Result<Option<Vec<SqlValue>>> {
    Ok(query_rows(stmt)?.into_iter().next())
}

/// Run a statement that returns no rows.
pub fn db_exec(conn: &Connection, sql: &str) -> rusqlite::Result<usize> {
    conn.execute(sql, [])
}

/// Borrow a `&[SqlValue]` as the `&[&dyn ToSql]` rusqlite's parameter APIs take.
///
/// The frankensqlite call sites all pass `&[SqliteValue::from(..), ..]`; rusqlite wants
/// `&[&dyn ToSql]`, and the two do not coerce. This is the single conversion point, so the
/// ~138 `execute_with_params` and ~91 `query_with_params` sites are a type-path change
/// rather than 229 separate rewrites.
///
/// The returned vector borrows from `values`, so it cannot outlive the call — which is
/// exactly the constraint the call sites already have.
pub fn params_from(values: &[SqlValue]) -> Vec<&dyn ToSql> {
    values.iter().map(|v| v as &dyn ToSql).collect()
}

/// [`query_rows`] with bind parameters.
pub fn query_rows_with_params<'stmt, P>(
    stmt: &'stmt mut Statement<'_>,
    params: P,
) -> rusqlite::Result<Vec<Vec<SqlValue>>>
where
    P: rusqlite::Params,
{
    let total = stmt.column_count();
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(column_values(row, 0, total)?);
    }
    Ok(out)
}

/// [`query_row_values`] with bind parameters.
pub fn query_row_values_with_params<P>(
    stmt: &mut Statement<'_>,
    params: P,
) -> rusqlite::Result<Option<Vec<SqlValue>>>
where
    P: rusqlite::Params,
{
    Ok(query_rows_with_params(stmt, params)?.into_iter().next())
}

/// Remap `QueryReturnedNoRows`.
///
/// frankensqlite's `query_row` returned the row directly; rusqlite returns
/// `Err(Error::QueryReturnedNoRows)`. The migration keeps the frankensqlite shape so the call
/// sites do not each grow a match arm, which means the "no row" condition stays a distinct
/// error rather than a sentinel value that could be confused with real data.
pub fn query_one<T>(stmt: &mut Statement<'_>) -> rusqlite::Result<T>
where
    T: FromSql,
{
    stmt.query_row([], |row| row.get(0))
}

/// Convert a rusqlite error into the crate's own error type.
///
/// The engine swap changed which error type reaches `BeadsError`, and Phase 2 gave
/// `BeadsError` a second variant so the tree keeps compiling. This helper is the seam: once
/// Phase 8 deletes `DatabaseLegacy`, every call site of this function becomes `?` again.
pub fn db_err(err: Error) -> crate::error::BeadsError {
    crate::error::BeadsError::Database(err)
}

/// `FromSql` for a borrowed `&str`, which rusqlite deliberately does not provide.
///
/// Needed by the row parsers, which read text columns. rusqlite's blanket impls make
/// `get::<String, _>` work but not `get::<&str, _>`, because the returned reference would
/// borrow from a temporary.
pub fn get_str(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    // `as_str_or_null` does the type check, the NULL check, and the UTF-8 validation, and
    // reports each failure distinctly. Returning `rusqlite::Result` rather than
    // `FromSqlResult` matters: `FromSqlError::InvalidType` is a unit variant in 0.40 and
    // carries neither the expected type nor the column index, so a caller that needs to say
    // WHICH column was wrong would be guessing. Here the index is known, so the error is
    // built where the context exists.
    let value = row.get_ref(idx)?;
    if value == ValueRef::Null {
        return Ok(None);
    }
    match value.as_str() {
        Ok(text) => Ok(Some(text.to_owned())),
        Err(_) => Err(rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            Box::new(FromSqlError::InvalidType),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("in-memory db");
        c.execute_batch("CREATE TABLE t (a INTEGER, b TEXT, c REAL, d BLOB, e)")
            .expect("create table");
        c
    }

    /// The `From` set is the whole point of this type. A missing impl turns 674 call sites
    /// into hand edits, so assert the set rather than trusting it.
    #[test]
    fn from_impls_cover_the_sqlite_value_set() {
        let values: Vec<SqlValue> = vec![
            SqlValue::from(1i64),
            SqlValue::from(1i32),
            SqlValue::from(1.5f64),
            SqlValue::from(String::from("s")),
            SqlValue::from("s"),
            SqlValue::from(std::sync::Arc::<str>::from("s")),
            SqlValue::from(vec![1u8]),
            SqlValue::from(&[1u8][..]),
            SqlValue::from(std::sync::Arc::<[u8]>::from(vec![1u8])),
        ];
        assert_eq!(values.len(), 9);
        assert!(values.contains(&SqlValue::from(1i64)));
    }

    /// Round-trip a value through a real database, so the `ToSql` impl is exercised rather
    /// than merely compiled.
    #[test]
    fn sql_value_binds_and_returns_every_storage_class() {
        let c = conn();
        c.execute(
            "INSERT INTO t (a, b, c, d, e) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                SqlValue::from(7i64),
                SqlValue::from("text"),
                SqlValue::from(2.5f64),
                SqlValue::from(vec![1u8, 2, 3]),
                SqlValue::from(0i64),
            ],
        )
        .expect("insert");

        let mut stmt = c.prepare("SELECT a, b, c, d, e FROM t").expect("prepare");
        let rows = query_rows(&mut stmt).expect("collect rows");
        assert_eq!(rows.len(), 1);

        let values = &rows[0];
        assert_eq!(values[0].as_integer(), Some(7));
        assert_eq!(values[1].as_text(), Some("text"));
        assert_eq!(values[2].as_float(), Some(2.5));
        assert_eq!(values[3].as_blob(), Some(&[1u8, 2, 3][..]));
    }

    /// The accessors must NOT coerce. A `Text` column read as an integer has to come back
    /// `None`, because silently coercing is the exact bug the golden snapshots exist to catch.
    /// The accessors must not coerce between storage classes.
    ///
    /// Note the non-numeric literal. Writing `'7'` into the `a INTEGER` column would NOT test
    /// this: SQLite column affinity converts a numeric-looking string into an integer *on
    /// insert*, so the value really would be stored as INTEGER 7 and the accessor would be
    /// reporting the truth. `'seven'` cannot be converted by affinity, so it stays TEXT and
    /// the accessor is genuinely being asked to not coerce.
    ///
    /// That affinity behaviour is itself worth knowing for this migration: a value that looks
    /// like text in a query can be stored as an integer, which is one of the ways a column's
    /// storage class can shift without any code changing.
    #[test]
    fn accessors_do_not_coerce_between_storage_classes() {
        let c = conn();
        c.execute("INSERT INTO t (a, b) VALUES ('seven', 'nine')", [])
            .expect("insert");

        let mut stmt = c.prepare("SELECT a, b FROM t").expect("prepare");
        let rows = query_rows(&mut stmt).expect("collect");
        let values = &rows[0];
        assert_eq!(values[0].as_integer(), None, "text must not read as integer");
        assert_eq!(values[0].as_text(), Some("seven"));
        assert_eq!(values[1].as_integer(), None);
        assert_eq!(values[1].as_float(), None);
    }

    /// Column affinity converts a numeric-looking string on insert. Asserted because the
    /// Phase 6 golden comparison depends on knowing this is the engine's behaviour and not a
    /// bug in the adapter.
    #[test]
    fn integer_column_affinity_converts_numeric_text_on_insert() {
        let c = conn();
        c.execute("INSERT INTO t (a) VALUES ('7')", []).expect("insert");
        let mut stmt = c.prepare("SELECT a FROM t").expect("prepare");
        let rows = query_rows(&mut stmt).expect("collect");
        assert_eq!(
            rows[0][0].as_integer(),
            Some(7),
            "INTEGER affinity stores '7' as an integer; this is SQLite, not the accessor"
        );
    }

    #[test]
    fn null_is_reported_not_guessed() {
        let c = conn();
        c.execute("INSERT INTO t (a) VALUES (NULL)", [])
            .expect("insert");
        let mut stmt = c.prepare("SELECT a FROM t").expect("prepare");
        let rows = query_rows(&mut stmt).expect("collect");
        let values = &rows[0];
        assert!(values[0].is_null());
        assert_eq!(values[0].as_integer(), None);
        assert_eq!(values[0].as_text(), None);
    }

    /// The eager collect is what lets row data outlive its statement, which is the reason
    /// `&Row` signatures are being changed to `&[SqlValue]`.
    #[test]
    fn collected_rows_outlive_the_statement_that_produced_them() {
        let c = conn();
        c.execute("INSERT INTO t (a) VALUES (1)", []).expect("insert");
        let rows = {
            let mut stmt = c.prepare("SELECT a FROM t").expect("prepare");
            query_rows(&mut stmt).expect("collect")
        };
        assert_eq!(rows[0][0].as_integer(), Some(1));
    }

    #[test]
    /// `get_str` operates on a borrowed `Row`, unlike the owned-value helpers, so it needs a
    /// live statement. It must reject a non-text column and report NULL rather than
    /// stringifying it.
    ///
    /// The column choices are load-bearing. The fixture table is
    /// `a INTEGER, b TEXT, c REAL, d BLOB, e`, and SQLite applies **column affinity on
    /// insert**: a `TEXT` column holding `5` is stored as the string `'5'`, so reading it back
    /// as text is correct, not a coercion. The genuinely non-text values here are `'hello'`
    /// surviving an INTEGER column and `2.5` in the REAL column.
    #[test]
    fn get_str_accepts_text_rejects_non_text_and_reports_null() {
        let c = conn();
        c.execute("INSERT INTO t (a, b, c, e) VALUES ('hello', 5, 2.5, NULL)", [])
            .expect("insert");
        let mut stmt = c.prepare("SELECT a, b, c, e FROM t").expect("prepare");
        let mut rows = stmt.query([]).expect("query");
        let row = rows.next().expect("one row").expect("no error");

        // 'hello' cannot be converted by INTEGER affinity, so it stays TEXT.
        assert_eq!(get_str(row, 0).expect("text"), Some("hello".to_string()));
        // 5 in a TEXT column is affinity-converted to '5' on insert: this is the engine, not
        // the accessor, and reading it as text is therefore the faithful answer.
        assert_eq!(get_str(row, 1).expect("affinity text"), Some("5".to_string()));
        // A REAL column is genuinely not text and must not stringify.
        assert!(get_str(row, 2).is_err(), "a real column must not stringify");
        assert_eq!(get_str(row, 3).expect("null column"), None);
    }

    #[test]
    fn db_exec_reports_affected_rows() {
        let c = conn();
        assert_eq!(db_exec(&c, "INSERT INTO t (a) VALUES (1)").expect("exec"), 1);
    }
}
