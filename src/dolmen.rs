use crate::Error;
use crate::db::DbConnection;
use dolmen::prelude::*;
use std::path::PathBuf;

impl From<dolmen::Error> for Error {
    fn from(e: dolmen::Error) -> Self {
        Self::new(e.message().clone().unwrap_or_default())
    }
}

impl From<Error> for dolmen::Error {
    fn from(e: Error) -> dolmen::Error {
        dolmen::Error::new(e.message().clone().unwrap_or_default())
    }
}

/// Add this plugin to a `Context` to add default
/// tables and basic database editing commands.
#[derive(Default, Clone)]
pub struct DbPlugin;

// TODO: is this plugin needed?
impl Plugin for DbPlugin {
    fn build(self, context: &mut Context) -> dolmen::Result<()> {
        context.add_resource(DbConfig::default());
        Ok(())
    }

    fn startup(context: &mut Context) -> dolmen::Result<()> {
        println!("asdf");
        let db_connection = if context
            .get_resource::<DbConfig>()
            .ok_or(dolmen::Error::default())?
            .open_db_in_memory
        {
            DbConnectionRes::open_test(context)
                .map_err(|e| dolmen::Error::new(e.message.unwrap_or_default()))
        } else {
            DbConnectionRes::open_default(context)
                .map_err(|e| dolmen::Error::new(e.message.unwrap_or_default()))
        }?;

        context.add_resource(db_connection);

        Ok(())
    }
}

#[derive(Resource)]
struct DbConnectionRes(crate::db::DbConnection);

impl DbConnectionRes {
    fn get_default_db_path() -> crate::Result<PathBuf> {
        // get the cache directory for this
        // application
        let dirs = directories::ProjectDirs::from("", "", "training_assistant")
            // translate to an error if it failed
            .ok_or(Error::new("Failed to get data directory"))?;

        Ok(dirs.data_dir().join("data/data.db"))
    }

    // opens a db connection at the default db path
    #[deprecated(note = "training_assistant specific, should be moved there")]
    pub(crate) fn open_default(context: &Context) -> crate::Result<Self> {
        assert!(!cfg!(test));
        let db_path = Self::get_default_db_path()?;
        let table_configs = match context.get_resource::<DbTableConfigsRes>() {
            Some(t) => t.0.clone(),
            None => Vec::new(),
        };
        Ok(Self(DbConnection::open_from_path(&db_path, table_configs)?))
    }

    // opens a db connection at a test db path
    #[deprecated(note = "training_assistant specific, should be moved there")]
    pub(crate) fn open_test(context: &Context) -> crate::Result<Self> {
        let table_configs = match context.get_resource::<DbTableConfigsRes>() {
            Some(t) => t.0.clone(),
            None => Vec::new(),
        };
        Ok(Self(DbConnection::open_in_memory(table_configs)?))
    }
}

#[derive(Resource, Default)]
struct DbTableConfigsRes(Vec<crate::db::TableConfig>);

impl DbTableConfigsRes {
    fn add_table(&mut self, config: crate::db::TableConfig) {
        self.0.push(config);
    }
}

/// An extension to `Context` to add database functionality acessible directly from the context.
pub trait DbContextExt {
    /// Gets the database connection from a `Context`, if it is active. Returns `Err` if not.
    fn db_connection(&mut self) -> crate::Result<&mut DbConnection>;

    /// Adds a new table configuration. Must be called before `Context::startup`.
    ///
    /// * `table` - The table configuration to add.
    fn add_table(&mut self, table: crate::db::TableConfig) -> &mut Context;
}

impl DbContextExt for Context {
    fn db_connection(&mut self) -> crate::Result<&mut DbConnection> {
        Ok(&mut self
            .get_resource_mut::<DbConnectionRes>()
            .ok_or(Error::new("no active db connection"))?
            .0)
    }

    fn add_table(&mut self, table: crate::db::TableConfig) -> &mut Context {
        if !self.has_resource::<DbTableConfigsRes>() {
            let mut configs = DbTableConfigsRes::default();
            configs.add_table(table);
            self.add_resource(configs);
        } else {
            // this unwrap is safe, as we know this resource exists
            self.get_resource_mut::<DbTableConfigsRes>()
                .unwrap()
                .add_table(table);
        }

        self
    }
}

#[derive(Resource, Default)]
pub struct DbConfig {
    pub open_db_in_memory: bool,
}

#[cfg(test)]
mod test {
    use crate::dolmen::{DbConfig, DbContextExt, DbPlugin};
    use crate::prelude::*;
    use crate::{Error, Result};
    use dolmen::prelude::*;
    extern crate self as reliquary;

    #[derive(Clone)]
    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn build(self, context: &mut Context) -> dolmen::Result<()> {
            context.add_table(TableConfig::new::<TestTableRow>("foo"));
            Ok(())
        }
    }

    #[derive(TableRow, Debug)]
    struct TestTableRow {
        bar: String,
    }

    #[test]
    fn db_table_ops_test() -> Result<()> {
        // create a context and add our test plugin
        let mut context = Context::new();
        context.add_plugin(TestPlugin)?;
        context.add_plugin(DbPlugin)?;
        context
            .get_resource_mut::<DbConfig>()
            .ok_or(Error::default())?
            .open_db_in_memory = true;

        context.startup()?;

        // open the db connection
        let mut db_connection = context.db_connection()?;

        // check the db connection is open
        assert!(db_connection.is_open());

        // test db connection shouldn't have a file path
        assert!(db_connection.db_path().is_none());

        // test field type info
        let field_types = (db_connection
            .tables()
            .iter()
            .find(|t| t.table_name == "foo")
            .ok_or(Error::default())?
            .field_types_fn)();
        assert_eq!(field_types.len(), 1);
        assert_eq!(field_types[0].name(), "bar");
        assert_eq!(field_types[0].type_id(), std::any::TypeId::of::<String>());

        // insert a row and check the inserted row is
        // row 1
        // (the table was empty)
        let inserted_row = db_connection.new_row_in_table("foo")?;
        assert_eq!(inserted_row.0, 1);

        // check the table row IDs returned are just
        // our newly created row
        let table_row_ids = db_connection.get_table_row_ids("foo")?;
        assert_eq!(table_row_ids, vec![1]);

        // set a field in the created row
        db_connection.set_field_in_table("foo", inserted_row, "bar", "foobar")?;

        // ensure the field matches
        let field = db_connection.get_field_in_table_row::<String>("foo", inserted_row, "bar")?;
        assert_eq!(field, "foobar");

        // get the table row and ensure the field matches
        let table_row =
            TestTableRow::from_table_row(&mut db_connection, "foo".into(), inserted_row)?;
        assert_eq!(table_row.bar, "foobar");

        // delete the row
        db_connection.remove_row_in_table("foo", inserted_row)?;

        // ensure the table row IDs are empty
        let table_row_ids_2 = db_connection.get_table_row_ids("foo")?;
        assert_eq!(table_row_ids_2.len(), 0);

        // delete the db. this one is in memory, so it
        // should just close the connection
        db_connection.delete_db()?;

        Ok(())
    }
}
