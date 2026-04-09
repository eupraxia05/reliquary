use std::fmt::Display;

use crate::db;
use bevy::prelude::*;

#[derive(Resource, Clone, Copy)]
pub enum DbSource {
    File { application_name: &'static str },
    Memory,
}

pub struct ReliquaryPlugin {
    db_source: DbSource,
}

impl Plugin for ReliquaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, startup.pipe(handle_system_result));
        app.insert_resource(self.db_source);
    }
}

#[derive(Debug)]
struct Error {
    message: &'static str,
}

unsafe impl Sync for Error {}
unsafe impl Send for Error {}

impl Error {
    fn new(msg: &'static str) -> Self {
        Self { message: msg }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub trait DbAppExt {
    fn db_connection(&mut self) -> Mut<db::DbConnection>;

    fn add_table(&mut self, table: db::TableConfig) -> &mut App;
}

impl DbAppExt for App {
    fn db_connection(&mut self) -> Mut<db::DbConnection> {
        self.world_mut().non_send_resource_mut::<db::DbConnection>()
    }

    fn add_table(&mut self, table: db::TableConfig) -> &mut App {
        self
    }
}

pub trait DbWorldExt {
    fn db_connection(&mut self) -> Mut<db::DbConnection>;
}

impl DbWorldExt for World {
    fn db_connection(&mut self) -> Mut<db::DbConnection> {
        self.non_send_resource_mut::<db::DbConnection>()
    }
}

fn startup(world: &mut World) -> Result<(), BevyError> {
    debug!("Reliquary startup...");

    let mut tables = world.query::<&mut Table>();

    let table_configs = tables
        .iter(world)
        .map(|t| t.config.clone())
        .collect::<Vec<_>>();

    let db_source = world
        .get_resource::<DbSource>()
        .ok_or(Error::new("no DbSource"))?;

    let db_connection = match db_source {
        DbSource::File { application_name } => {
            let dirs = directories::ProjectDirs::from("", "", application_name)
                .ok_or(Error::new("lmao"))?;

            let path = dirs.project_path().join("data.db");

            crate::db::DbConnection::open_from_path(&path, table_configs)?
        }
        DbSource::Memory => crate::db::DbConnection::open_in_memory(table_configs)?,
    };

    debug!("adding resource...");
    world.insert_non_send_resource(db_connection);

    Ok(())
}

fn handle_system_result(In(result): In<Result<()>>) {
    match result {
        Err(e) => {
            error!("Reliquary error: {}", e.to_string());
            #[cfg(test)]
            panic!("System error raised, stopping test...");
        }
        _ => {}
    }
}

#[derive(Component)]
struct Table {
    config: db::TableConfig,
}

#[cfg(test)]
mod test {
    use crate::bevy::DbAppExt;
    use crate::bevy::DbWorldExt;
    use crate::bevy::ReliquaryPlugin;
    use crate::bevy::Table;
    use crate::prelude::*;

    extern crate self as reliquary;
    use bevy::ecs::schedule::LogLevel;
    use bevy::log::Level;
    use bevy::prelude::*;

    #[derive(TableRow, Debug)]
    struct TestTableRow {
        foo: String,
    }

    fn verify_test_1(world: &mut World) {
        assert!(world.db_connection().tables().len() == 1);
    }

    #[test]
    fn test_1() {
        let mut app = App::new();
        app.add_plugins(ReliquaryPlugin {
            db_source: crate::bevy::DbSource::Memory,
        });
        app.add_plugins(bevy::log::LogPlugin {
            level: Level::DEBUG,
            ..default()
        });
        app.world_mut().spawn(Table {
            config: TableConfig::new::<TestTableRow>("test"),
        });
        app.add_systems(PostStartup, verify_test_1);
        app.run();
    }
}
