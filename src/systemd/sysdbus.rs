//! Dbus abstraction
//! Documentation can be found at https://www.freedesktop.org/wiki/Software/systemd/dbus/
pub(super) mod watcher;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use log::{debug, info, trace, warn};

use serde::Deserialize;

use zbus::{
    Message,
    blocking::{Connection, MessageIterator, Proxy, fdo},
    message::Flags,
    names::InterfaceName,
    proxy,
};

use zvariant::{Array, DynamicType, ObjectPath, OwnedValue, Str, Type};

use crate::{
    systemd::{
        data::{EnableUnitFilesReturn, UnitInfo},
        enums::{ActiveState, UnitType},
    },
    widget::preferences::data::{DbusLevel, PREFERENCES},
};

use super::{
    Dependency, SystemdErrors, SystemdUnitFile, UpdatedUnitInfo,
    data::DisEnAbleUnitFiles,
    enums::{
        DependencyType, DisEnableFlags, EnablementStatus, KillWho, StartStopMode, UnitDBusLevel,
    },
};

pub(crate) const DESTINATION_SYSTEMD: &str = "org.freedesktop.systemd1";
pub(super) const INTERFACE_SYSTEMD_UNIT: &str = "org.freedesktop.systemd1.Unit";
pub(super) const INTERFACE_SYSTEMD_MANAGER: &str = "org.freedesktop.systemd1.Manager";
pub(crate) const PATH_SYSTEMD: &str = "/org/freedesktop/systemd1";

const METHOD_LIST_UNIT: &str = "ListUnits";

const METHOD_LIST_UNIT_FILES: &str = "ListUnitFiles";

const METHOD_START_UNIT: &str = "StartUnit";
const METHOD_STOP_UNIT: &str = "StopUnit";
const METHOD_RESTART_UNIT: &str = "RestartUnit";
const METHOD_GET_UNIT_FILE_STATE: &str = "GetUnitFileState";
const METHOD_KILL_UNIT: &str = "KillUnit";
const METHOD_QUEUE_SIGNAL_UNIT: &str = "QueueSignalUnit";
const METHOD_CLEAN_UNIT: &str = "CleanUnit";
const METHOD_MASK_UNIT_FILES: &str = "MaskUnitFiles";
const METHOD_UNMASK_UNIT_FILES: &str = "UnmaskUnitFiles";
const METHOD_GET_UNIT: &str = "GetUnit";
const METHOD_ENABLE_UNIT_FILES: &str = "EnableUnitFilesWithFlags";
const METHOD_DISABLE_UNIT_FILES: &str = "DisableUnitFilesWithFlags";
pub const METHOD_RELOAD: &str = "Reload";
pub const METHOD_GET_UNIT_PROCESSES: &str = "GetUnitProcesses";
pub const METHOD_FREEZE_UNIT: &str = "FreezeUnit";
pub const METHOD_THAW_UNIT: &str = "ThawUnit";
pub const METHOD_RELOAD_UNIT: &str = "ReloadUnit";

const METHOD_PRESET_UNIT_FILES: &str = "PresetUnitFiles";
const METHOD_LINK_UNIT_FILES: &str = "LinkUnitFiles";
const METHOD_REENABLE_UNIT_FILES: &str = "ReenableUnitFiles";

#[derive(Deserialize, Type, PartialEq, Debug)]
struct LUnitFiles<'a> {
    primary_unit_name: &'a str,
    enablement_status: &'a str,
}

#[derive(Deserialize, Type, PartialEq, Debug)]
pub struct LUnit<'a> {
    pub primary_unit_name: &'a str,
    pub description: &'a str,
    pub load_state: &'a str,
    pub active_state: &'a str,
    pub sub_state: &'a str,
    pub followed_unit: &'a str,
    #[serde(borrow)]
    pub unit_object_path: ObjectPath<'a>,
    ///If there is a job queued for the job unit the numeric job id, 0 otherwise
    pub numeric_job_id: u32,
    pub job_type: &'a str,
    pub job_object_path: ObjectPath<'a>,
}

fn get_connection(level: UnitDBusLevel) -> Result<Connection, SystemdErrors> {
    debug!("Level {:?}, id {}", level, level as u32);
    let connection_builder = match level {
        UnitDBusLevel::UserSession => zbus::blocking::connection::Builder::session()?,
        UnitDBusLevel::System => zbus::blocking::connection::Builder::system()?,
    };

    let connection = connection_builder
        .auth_mechanism(zbus::AuthMechanism::External)
        .build()?;

    //println!("connection {:#?}", connection);

    Ok(connection)
}

async fn get_connection_async(level: UnitDBusLevel) -> Result<zbus::Connection, SystemdErrors> {
    debug!("Level {:?}, id {}", level, level as u32);
    let connection_builder = match level {
        UnitDBusLevel::UserSession => zbus::connection::Builder::session()?,
        UnitDBusLevel::System => zbus::connection::Builder::system()?,
    };

    let connection = connection_builder
        .auth_mechanism(zbus::AuthMechanism::External)
        .build()
        .await?;

    //println!("connection {:#?}", connection);

    Ok(connection)
}

async fn list_units_description_conn_async(
    connection: Arc<zbus::Connection>,
    dbus_level: UnitDBusLevel,
) -> Result<HashMap<String, UnitInfo>, SystemdErrors> {
    let message = connection
        .call_method(
            Some(DESTINATION_SYSTEMD),
            PATH_SYSTEMD,
            Some(INTERFACE_SYSTEMD_MANAGER),
            METHOD_LIST_UNIT,
            &(),
        )
        .await?;

    fill_list_units_description(dbus_level, message)
}

fn fill_list_units_description(
    dbus_level: UnitDBusLevel,
    message: Message,
) -> Result<HashMap<String, UnitInfo>, SystemdErrors> {
    let body = message.body();

    let array: Vec<LUnit> = body.deserialize()?;

    let mut map: HashMap<String, UnitInfo> = HashMap::new();

    for listed_unit in array.iter() {
        let unit = UnitInfo::from_listed_unit(listed_unit, dbus_level);

        map.insert(unit.primary(), unit);
    }

    Ok(map)
}

/// Returns the current enablement status of the unit
pub fn get_unit_file_state(
    level: UnitDBusLevel,
    unit_file: &str,
) -> Result<EnablementStatus, SystemdErrors> {
    let connection = get_connection(level)?;

    let message = connection.call_method(
        Some(DESTINATION_SYSTEMD),
        PATH_SYSTEMD,
        Some(INTERFACE_SYSTEMD_MANAGER),
        METHOD_GET_UNIT_FILE_STATE,
        &(unit_file),
    )?;

    let body = message.body();
    let enablement_status: &str = body.deserialize()?;

    Ok(EnablementStatus::from_str(enablement_status))
}

pub async fn get_unit_file_state_async(
    connection: &zbus::Connection,
    unit_file: &str,
) -> Result<EnablementStatus, SystemdErrors> {
    let message = connection
        .call_method(
            Some(DESTINATION_SYSTEMD),
            PATH_SYSTEMD,
            Some(INTERFACE_SYSTEMD_MANAGER),
            METHOD_GET_UNIT_FILE_STATE,
            &(unit_file),
        )
        .await?;

    let body = message.body();
    let enablement_status: &str = body.deserialize()?;

    Ok(EnablementStatus::from_str(enablement_status))
}

pub async fn list_units_description_and_state_async(
    level: UnitDBusLevel,
) -> Result<(HashMap<String, UnitInfo>, Vec<SystemdUnitFile>), SystemdErrors> {
    let connection = get_connection_async(level).await?;
    let conn = Arc::new(connection);
    let t1 = tokio::spawn(list_units_description_conn_async(conn.clone(), level));
    let t2 = tokio::spawn(list_unit_files_async(conn, level));

    let joined = tokio::join!(t1, t2);

    let units_map = joined.0??;
    let unit_files = joined.1??;

    Ok((units_map, unit_files))
}

pub async fn list_all_units()
-> Result<(HashMap<String, UnitInfo>, Vec<SystemdUnitFile>), SystemdErrors> {
    match PREFERENCES.dbus_level() {
        DbusLevel::UserSession => {
            list_units_description_and_state_async(UnitDBusLevel::UserSession).await
        }
        DbusLevel::System => list_units_description_and_state_async(UnitDBusLevel::System).await,
        DbusLevel::SystemAndSession => {
            let mut vec1 =
                list_units_description_and_state_async(UnitDBusLevel::UserSession).await?;
            let vec2 = list_units_description_and_state_async(UnitDBusLevel::System).await?;
            vec1.1.extend(vec2.1);
            vec1.0.extend(vec2.0);
            Ok(vec1)
        }
    }
}

pub async fn complete_unit_information(
    units: &Vec<(String, UnitDBusLevel, Option<String>)>,
) -> Result<Vec<UpdatedUnitInfo>, SystemdErrors> {
    let mut connection_system = None;
    let mut connection_session = None;

    let mut ouput = Vec::with_capacity(units.len());
    for (unit_primary, dbus_level, object_path) in units {
        let connection = match dbus_level {
            UnitDBusLevel::System => {
                if let Some(conn) = &connection_system {
                    conn
                } else {
                    let conn = get_connection_async(*dbus_level).await?;
                    connection_system.get_or_insert(conn) as &zbus::Connection
                }
            }
            UnitDBusLevel::UserSession => {
                if let Some(conn) = &connection_session {
                    conn
                } else {
                    let conn = get_connection_async(*dbus_level).await?;
                    connection_session.get_or_insert(conn) as &zbus::Connection
                }
            }
        };

        let f1 = complete_unit_info(unit_primary, object_path, connection);
        let f2 = get_unit_file_state_async(connection, unit_primary);

        let (r1, r2) = tokio::join!(f1, f2);

        match r1 {
            Ok(mut updated_unit_info) => {
                updated_unit_info.enablement_status = r2.ok();
                ouput.push(updated_unit_info)
            }
            Err(error) => warn!("Complete unit \"{}\" error {:?}", unit_primary, error),
        }
    }
    Ok(ouput)
}

#[proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait ZUnitInfo {
    #[zbus(property)]
    fn id(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn description(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn load_state(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn active_state(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn sub_state(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn following(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn fragment_path(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn unit_file_state(&self) -> Result<String, zbus::Error>;

    #[zbus(property)]
    fn unit_file_preset(&self) -> Result<String, zbus::Error>;
}

macro_rules! fill_completing_info {
    ($update:expr, $unit_info_proxy:expr, $f:ident) => {
        match $unit_info_proxy.$f().await {
            Ok(s) => {
                $update.$f = Some(s);
            }
            Err(err) => {
                let err: SystemdErrors = err.into();
                //warn!("Complete info Error: {:?}", err);
                return Err(err);
            }
        }
    };
}

async fn complete_unit_info(
    unit_primary: &str,
    object_path: &Option<String>,
    connection: &zbus::Connection,
) -> Result<UpdatedUnitInfo, SystemdErrors> {
    let object_path = match object_path {
        Some(o) => o.clone(),
        None => unit_dbus_path_from_name(unit_primary),
    };

    let unit_info_proxy = ZUnitInfoProxy::builder(connection)
        .path(object_path.clone())?
        .build()
        .await?;

    let mut update = UpdatedUnitInfo::new(unit_primary.to_owned(), object_path);

    if let Err(error) = fill_update(unit_info_proxy, &mut update).await {
        debug!("Complete info Error: {:?}", error);
    }

    Ok(update)
}

async fn fill_update(
    unit_info_proxy: ZUnitInfoProxy<'_>,
    update: &mut UpdatedUnitInfo,
) -> Result<(), SystemdErrors> {
    let active_state = unit_info_proxy.active_state().await?;
    let active_state: ActiveState = active_state.as_str().into();
    update.active_state = Some(active_state);
    fill_completing_info!(update, unit_info_proxy, description);
    fill_completing_info!(update, unit_info_proxy, load_state);
    fill_completing_info!(update, unit_info_proxy, sub_state);
    fill_completing_info!(update, unit_info_proxy, unit_file_preset);
    fill_completing_info!(update, unit_info_proxy, fragment_path);
    update.valid_unit_name = true;

    Ok(())
}

/// Communicates with dbus to obtain a list of unit files and returns them as a `Vec<SystemdUnit>`.
#[allow(dead_code)]
pub fn list_unit_files(
    connection: &Connection,
    level: UnitDBusLevel,
) -> Result<Vec<SystemdUnitFile>, SystemdErrors> {
    let message = connection.call_method(
        Some(DESTINATION_SYSTEMD),
        PATH_SYSTEMD,
        Some(INTERFACE_SYSTEMD_MANAGER),
        METHOD_LIST_UNIT_FILES,
        &(),
    )?;

    fill_list_unit_files(message, level)
}

fn fill_list_unit_files(
    message: Message,
    level: UnitDBusLevel,
) -> Result<Vec<SystemdUnitFile>, SystemdErrors> {
    let body = message.body();

    let array: Vec<LUnitFiles> = body.deserialize()?;

    let mut systemd_units: Vec<SystemdUnitFile> = Vec::with_capacity(array.len());

    for unit_file in array.iter() {
        let Some((_prefix, full_name)) = unit_file.primary_unit_name.rsplit_once('/') else {
            return Err(SystemdErrors::Malformed(
                "rsplit_once(\"/\")".to_string(),
                unit_file.primary_unit_name.to_owned(),
            ));
        };

        /*         let Some((name, system_type)) = full_name.rsplit_once('.') else {
            return Err(SystemdErrors::Malformed(
                "rsplit_once('.')".to_owned(),
                full_name.to_owned(),
            ));
        }; */

        let status_code = EnablementStatus::from_str(unit_file.enablement_status);
        //let utype = UnitType::new(system_type);

        systemd_units.push(SystemdUnitFile {
            full_name: full_name.to_owned(),
            status_code,
            level,
            path: unit_file.primary_unit_name.to_owned(),
        });
    }

    Ok(systemd_units)
}

/// Communicates with dbus to obtain a list of unit files and returns them as a `Vec<SystemdUnit>`.
pub async fn list_unit_files_async(
    connection: Arc<zbus::Connection>,
    level: UnitDBusLevel,
) -> Result<Vec<SystemdUnitFile>, SystemdErrors> {
    let message = connection
        .call_method(
            Some(DESTINATION_SYSTEMD),
            PATH_SYSTEMD,
            Some(INTERFACE_SYSTEMD_MANAGER),
            METHOD_LIST_UNIT_FILES,
            &(),
        )
        .await?;

    fill_list_unit_files(message, level)
}

/// Takes a unit name as input and attempts to start it
pub(super) fn start_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    mode: StartStopMode,
) -> Result<String, SystemdErrors> {
    send_disenable_message(
        level,
        METHOD_START_UNIT,
        &(unit_name, mode.as_str()),
        handle_start_stop_answer,
    )
}

fn handle_start_stop_answer(
    method: &str,
    return_message: &Message,
) -> Result<String, SystemdErrors> {
    let body = return_message.body();

    let job_path: zvariant::ObjectPath = body.deserialize()?;

    let created_job_object = job_path.to_string();
    info!("{method} SUCCESS, response job id {created_job_object}");

    Ok(created_job_object)
}

/// Takes a unit name as input and attempts to stop it.
pub(super) fn stop_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    mode: StartStopMode,
) -> Result<String, SystemdErrors> {
    send_disenable_message(
        level,
        METHOD_STOP_UNIT,
        &(unit_name, mode.as_str()),
        handle_start_stop_answer,
    )
}

/// Enqeues a start job, and possibly depending jobs.
pub(super) fn restart_unit(
    level: UnitDBusLevel,
    unit: &str,
    mode: StartStopMode,
) -> Result<String, SystemdErrors> {
    send_disenable_message(
        level,
        METHOD_RESTART_UNIT,
        &(unit, mode.as_str()),
        handle_start_stop_answer,
    )
}

pub(super) fn enable_unit_files(
    level: UnitDBusLevel,
    unit_names_or_files: &[&str],
    flags: DisEnableFlags,
) -> Result<EnableUnitFilesReturn, SystemdErrors> {
    fn handle_answer(
        _method: &str,
        return_message: &Message,
    ) -> Result<EnableUnitFilesReturn, SystemdErrors> {
        let body = return_message.body();

        let return_msg: EnableUnitFilesReturn = body.deserialize()?;

        info!("Enable unit files {:?}", return_msg);

        Ok(return_msg)
    }

    send_disenable_message(
        level,
        METHOD_ENABLE_UNIT_FILES,
        &(unit_names_or_files, flags.as_u64()),
        handle_answer,
    )
}

pub(super) fn disable_unit_files(
    level: UnitDBusLevel,
    unit_names_or_files: &[&str],
    flags: DisEnableFlags,
) -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
    fn handle_answer(
        _method: &str,
        return_message: &Message,
    ) -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
        let body = return_message.body();

        let return_msg: Vec<DisEnAbleUnitFiles> = body.deserialize()?;

        info!("Disable unit files {:?}", return_msg);

        Ok(return_msg)
    }

    send_disenable_message(
        level,
        METHOD_DISABLE_UNIT_FILES,
        &(unit_names_or_files, flags.as_u64()),
        handle_answer,
    )
}

fn send_disenable_message<T, U>(
    level: UnitDBusLevel,
    method: &str,
    body: &T,
    handler: impl Fn(&str, &Message) -> Result<U, SystemdErrors>,
) -> Result<U, SystemdErrors>
where
    T: serde::ser::Serialize + DynamicType,
    U: std::fmt::Debug,
{
    let message = Message::method_call(PATH_SYSTEMD, method)?
        .with_flags(Flags::AllowInteractiveAuth)?
        .destination(DESTINATION_SYSTEMD)?
        .interface(INTERFACE_SYSTEMD_MANAGER)?
        .build(body)?;

    let connection = get_connection(level)?;

    connection.send(&message)?;

    let message_it = MessageIterator::from(connection);

    for message_res in message_it {
        debug!("Message response {:?}", message_res);
        let return_message = message_res?;

        match return_message.message_type() {
            zbus::message::Type::MethodReturn => {
                info!("{method} Response");
                let result = handler(method, &return_message);
                return result;
            }
            zbus::message::Type::MethodCall => {
                warn!("Not supposed to happen: {:?}", return_message);
                break;
            }
            zbus::message::Type::Error => {
                let zb_error = zbus::Error::from(return_message);

                {
                    match zb_error {
                        zbus::Error::MethodError(
                            ref owned_error_name,
                            ref details,
                            ref message,
                        ) => {
                            warn!(
                                "Method error: {}\nDetails: {}\n{:?}",
                                owned_error_name.as_str(),
                                details.as_ref().map(|s| s.as_str()).unwrap_or_default(),
                                message
                            )
                        }
                        _ => warn!("Bus error: {:?}", zb_error),
                    }
                }
                let error = SystemdErrors::from((zb_error, method));
                return Err(error);
            }
            zbus::message::Type::Signal => {
                info!("Signal: {:?}", return_message);
                continue;
            }
        }
    }

    let msg = format!("{:?} ????, response supposed to be Unreachable", method);
    warn!("{}", msg);
    Err(SystemdErrors::Malformed(
        msg,
        "sequences of messages".to_owned(),
    ))
}

#[allow(dead_code)]
fn get_unit_object_path_connection(
    unit_name: &str,
    connection: &Connection,
) -> Result<ObjectPath<'static>, SystemdErrors> {
    let message = connection.call_method(
        Some(DESTINATION_SYSTEMD),
        PATH_SYSTEMD,
        Some(INTERFACE_SYSTEMD_MANAGER),
        METHOD_GET_UNIT,
        &(unit_name),
    )?;

    let body = message.body();

    let object_path: zvariant::ObjectPath = body.deserialize()?;

    Ok(object_path.to_owned())
}

pub fn reload_all_units(level: UnitDBusLevel) -> Result<(), SystemdErrors> {
    //let handler_cloned: = handler;

    send_disenable_message(level, METHOD_RELOAD, &(), move |method, _message| {
        info!("{method} SUCCESS");
        Ok(())
    })
}

pub(super) fn kill_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    mode: KillWho,
    signal: i32,
) -> Result<(), SystemdErrors> {
    let handler = |_method: &str, _return_message: &Message| -> Result<(), SystemdErrors> {
        info!(
            "Kill Unit {} mode {} signal {} SUCCESS",
            unit_name, mode, signal
        );
        Ok(())
    };

    send_disenable_message(
        level,
        METHOD_KILL_UNIT,
        &(unit_name, mode.as_str(), signal),
        handler,
    )
}

pub(super) fn freeze_unit(level: UnitDBusLevel, unit_name: &str) -> Result<(), SystemdErrors> {
    let handler = |_method: &str, _return_message: &Message| -> Result<(), SystemdErrors> {
        info!("Freeze Unit {} SUCCESS", unit_name);
        Ok(())
    };

    send_disenable_message(level, METHOD_FREEZE_UNIT, &(unit_name), handler)
}

pub(super) fn thaw_unit(level: UnitDBusLevel, unit_name: &str) -> Result<(), SystemdErrors> {
    let handler = |_method: &str, _return_message: &Message| -> Result<(), SystemdErrors> {
        info!("Thaw Unit {} SUCCESS", unit_name);
        Ok(())
    };

    send_disenable_message(level, METHOD_THAW_UNIT, &(unit_name), handler)
}

pub(super) fn preset_unit_file(
    level: UnitDBusLevel,
    files: &[&str],
    runtime: bool,
    force: bool,
) -> Result<EnableUnitFilesReturn, SystemdErrors> {
    let handler =
        |_method: &str, return_message: &Message| -> Result<EnableUnitFilesReturn, SystemdErrors> {
            let body = return_message.body();

            let return_msg: EnableUnitFilesReturn = body.deserialize()?;

            info!("Preset Unit Files {:?} SUCCESS", files);
            Ok(return_msg)
        };

    send_disenable_message(
        level,
        METHOD_PRESET_UNIT_FILES,
        &(files, runtime, force),
        handler,
    )
}

pub(super) fn link_unit_files(
    level: UnitDBusLevel,
    files: &[&str],
    runtime: bool,
    force: bool,
) -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
    let handler = |_method: &str,
                   return_message: &Message|
     -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
        let body = return_message.body();

        let return_msg: Vec<DisEnAbleUnitFiles> = body.deserialize()?;

        info!("Link Unit Files {:?} SUCCESS", files);
        Ok(return_msg)
    };
    send_disenable_message(
        level,
        METHOD_LINK_UNIT_FILES,
        &(files, runtime, force),
        handler,
    )
}

pub(super) fn reenable_unit_file(
    level: UnitDBusLevel,
    files: &[&str],
    runtime: bool,
    force: bool,
) -> Result<EnableUnitFilesReturn, SystemdErrors> {
    let handler =
        |_method: &str, return_message: &Message| -> Result<EnableUnitFilesReturn, SystemdErrors> {
            let body = return_message.body();

            let return_msg: EnableUnitFilesReturn = body.deserialize()?;

            info!("Reenable Unit Files {:?} SUCCESS", files);
            Ok(return_msg)
        };
    send_disenable_message(
        level,
        METHOD_REENABLE_UNIT_FILES,
        &(files, runtime, force),
        handler,
    )
}

pub(super) fn reload_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    mode: &str,
) -> Result<(), SystemdErrors> {
    let handler = |_method: &str, _return_message: &Message| -> Result<(), SystemdErrors> {
        info!("Reload Unit SUCCESS");
        Ok(())
    };

    send_disenable_message(level, METHOD_RELOAD_UNIT, &(unit_name, mode), handler)
}

pub(super) fn queue_signal_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    mode: KillWho,
    signal: i32,
    value: i32,
) -> Result<(), SystemdErrors> {
    fn handle_answer(_method: &str, _return_message: &Message) -> Result<(), SystemdErrors> {
        info!("Queue Signal SUCCESS");

        Ok(())
    }

    send_disenable_message(
        level,
        METHOD_QUEUE_SIGNAL_UNIT,
        &(unit_name, mode.as_str(), signal, value),
        handle_answer,
    )
}

pub(super) fn clean_unit(
    level: UnitDBusLevel,
    unit_name: &str,
    what: &[&str],
) -> Result<(), SystemdErrors> {
    let handle_answer = |_method: &str, _return_message: &Message| {
        info!("Clean Unit {} {:?} SUCCESS", unit_name, what);

        Ok(())
    };

    send_disenable_message(level, METHOD_CLEAN_UNIT, &(unit_name, what), handle_answer)
}

pub(super) fn mask_unit_files(
    level: UnitDBusLevel,
    files: &[&str],
    runtime: bool,
    force: bool,
) -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
    let handle_answer = |_method: &str,
                         return_message: &Message|
     -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
        info!(
            "Mask Unit File {:?} runtime {:?} force {:?} SUCCESS",
            files, runtime, force
        );

        let body = return_message.body();

        let return_msg: Vec<DisEnAbleUnitFiles> = body.deserialize()?;

        info!("Mask Unit File {:?}", return_msg);

        Ok(return_msg)
    };

    send_disenable_message(
        level,
        METHOD_MASK_UNIT_FILES,
        &(files, runtime, force),
        handle_answer,
    )
}

pub(super) fn unmask_unit_files(
    level: UnitDBusLevel,
    files: &[&str],
    runtime: bool,
) -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
    let handle_answer = |_method: &str,
                         return_message: &Message|
     -> Result<Vec<DisEnAbleUnitFiles>, SystemdErrors> {
        info!(
            "UnMask Unit File {:?} runtime {:?}  SUCCESS",
            files, runtime
        );

        let body = return_message.body();

        let return_msg: Vec<DisEnAbleUnitFiles> = body.deserialize()?;

        info!("UnMask Unit File {:?}", return_msg);

        Ok(return_msg)
    };

    send_disenable_message(
        level,
        METHOD_UNMASK_UNIT_FILES,
        &(files, runtime),
        handle_answer,
    )
}

fn convert_to_string(value: &zvariant::Value) -> String {
    match value {
        zvariant::Value::U8(i) => i.to_string(),
        zvariant::Value::Bool(b) => b.to_string(),
        zvariant::Value::I16(i) => i.to_string(),
        zvariant::Value::U16(i) => i.to_string(),
        zvariant::Value::I32(i) => i.to_string(),
        zvariant::Value::U32(i) => i.to_string(),
        zvariant::Value::I64(i) => i.to_string(),
        zvariant::Value::U64(i) => i.to_string(),
        zvariant::Value::F64(i) => i.to_string(),
        zvariant::Value::Str(s) => s.to_string(),
        zvariant::Value::Signature(s) => s.to_string(),
        zvariant::Value::ObjectPath(op) => op.to_string(),
        zvariant::Value::Value(v) => v.to_string(),
        zvariant::Value::Array(a) => {
            let mut d_str = String::from("[ ");

            let mut it = a.iter().peekable();
            while let Some(mi) = it.next() {
                d_str.push_str(&convert_to_string(mi));
                if it.peek().is_some() {
                    d_str.push_str(", ");
                }
            }

            d_str.push_str(" ]");
            d_str
        }
        zvariant::Value::Dict(d) => {
            let mut d_str = String::from("{ ");
            for (mik, miv) in d.iter() {
                d_str.push_str(&convert_to_string(mik));
                d_str.push_str(" : ");
                d_str.push_str(&convert_to_string(miv));
            }
            d_str.push_str(" }");
            d_str
        }
        zvariant::Value::Structure(stc) => {
            let mut d_str = String::from("{ ");

            let mut it = stc.fields().iter().peekable();
            while let Some(mi) = it.next() {
                d_str.push_str(&convert_to_string(mi));
                if it.peek().is_some() {
                    d_str.push_str(", ");
                }
            }

            d_str.push_str(" }");
            d_str
        }
        zvariant::Value::Fd(fd) => fd.to_string(),
        //zvariant::Value::Maybe(maybe) => maybe.to_string(),
    }
}

pub fn fetch_system_info(level: UnitDBusLevel) -> Result<BTreeMap<String, String>, SystemdErrors> {
    let res = change_p(level);
    warn!("res {:?}", res);
    fetch_system_unit_info(level, PATH_SYSTEMD, UnitType::Manager)
}

pub fn fetch_system_unit_info(
    level: UnitDBusLevel,
    object_path: &str,
    unit_type: UnitType,
) -> Result<BTreeMap<String, String>, SystemdErrors> {
    let properties: HashMap<String, OwnedValue> =
        fetch_system_unit_info_native(level, object_path, unit_type)?;

    let mut map = BTreeMap::new();

    for (key, value) in properties.into_iter() {
        trace!("{:?} {:?}", key, value);

        let str_val = convert_to_string(&value);
        map.insert(key.to_owned(), str_val);
    }

    Ok(map)
}

fn change_p(level: UnitDBusLevel) -> Result<(), SystemdErrors> {
    let method = "Get";

    let body = ("org.freedesktop.systemd1.Manager", "LogLevel");

    let message = Message::method_call(PATH_SYSTEMD, method)?
        .destination(DESTINATION_SYSTEMD)?
        .interface("org.freedesktop.DBus.Properties")?
        .with_flags(Flags::AllowInteractiveAuth)?
        .build(&body)?;

    let connection = get_connection(level)?;

    connection.send(&message)?;

    let message_it = MessageIterator::from(connection);

    for message_res in message_it {
        debug!("Message response {:?}", message_res);
        let return_message = message_res?;

        match return_message.message_type() {
            zbus::message::Type::MethodReturn => {
                info!("{method} Response");

                let body = return_message.body();

                let return_msg: OwnedValue = body.deserialize()?;

                info!("{method} Response {:?}", return_msg);

                //  let result = handler(method, &return_message);
                return Ok(());
            }
            zbus::message::Type::MethodCall => {
                warn!("Not supposed to happen: {:?}", return_message);
                break;
            }
            zbus::message::Type::Error => {
                let error = zbus::Error::from(return_message);
                return Err(SystemdErrors::from(error));
            }
            zbus::message::Type::Signal => {
                info!("Signal: {:?}", return_message);
                continue;
            }
        }
    }

    let msg = format!("{:?} ????, response supposed to be Unreachable", method);
    warn!("{}", msg);
    Err(SystemdErrors::Malformed(
        msg,
        "sequences of messages".to_owned(),
    ))
}

pub fn fetch_system_unit_info_native(
    level: UnitDBusLevel,
    object_path: &str,
    unit_type: UnitType,
) -> Result<HashMap<String, OwnedValue>, SystemdErrors> {
    let connection = get_connection(level)?;

    debug!("Unit path: {object_path}");
    let properties_proxy: zbus::blocking::fdo::PropertiesProxy =
        fdo::PropertiesProxy::builder(&connection)
            .destination(DESTINATION_SYSTEMD)?
            .path(object_path)?
            .build()?;

    let unit_interface = unit_type.interface();

    let interface_name = InterfaceName::try_from(unit_interface).unwrap();

    let mut properties: HashMap<String, OwnedValue> = properties_proxy.get_all(interface_name)?;

    if unit_type.extends_unit() {
        let unit_interface_name = InterfaceName::try_from(INTERFACE_SYSTEMD_UNIT).unwrap();

        let unit_properties: HashMap<String, OwnedValue> =
            properties_proxy.get_all(unit_interface_name)?;

        properties.extend(unit_properties);
    }

    trace!("properties {:?}", properties);
    Ok(properties)
}

pub fn fetch_unit(
    level: UnitDBusLevel,
    unit_primary_name: &str,
) -> Result<UnitInfo, SystemdErrors> {
    let connection = get_connection(level)?;

    let object_path = unit_dbus_path_from_name(unit_primary_name);

    debug!("path {object_path}");

    let properties_proxy = ZUnitInfoProxyBlocking::builder(&connection)
        .destination(DESTINATION_SYSTEMD)?
        .path(object_path.clone())?
        .build()?;

    let primary = properties_proxy.id()?;
    let description = properties_proxy.description().unwrap_or_default();
    let load_state = properties_proxy.load_state().unwrap_or_default();
    let active_state_str = properties_proxy.active_state().unwrap_or_default();
    let sub_state = properties_proxy.sub_state().unwrap_or_default();
    let followed_unit = properties_proxy.following().unwrap_or_default();

    let listed_unit = LUnit {
        primary_unit_name: &primary,
        description: &description,
        load_state: &load_state,
        active_state: &active_state_str,
        sub_state: &sub_state,
        followed_unit: &followed_unit,
        unit_object_path: ObjectPath::from_string_unchecked(object_path),
        numeric_job_id: 0,
        job_type: "",
        job_object_path: ObjectPath::from_static_str_unchecked(""),
    };

    let unit = UnitInfo::from_listed_unit(&listed_unit, level);

    if let Ok(fragment_path) = properties_proxy.fragment_path() {
        unit.set_file_path(Some(fragment_path));
    }

    /*     match get_unit_file_state(level, unit_primary_name) {
        Ok(unit_file_status) => unit.set_enable_status(unit_file_status as u8),
        Err(err) => warn!("Fail to get unit file state : {:?}", err),
    } */

    Ok(unit)
}

pub(super) fn unit_get_dependencies(
    dbus_level: UnitDBusLevel,
    unit_name: &str,
    unit_object_path: &str,
    dependency_type: DependencyType,
    plain: bool,
) -> Result<Dependency, SystemdErrors> {
    let connection = get_connection(dbus_level)?;
    let dependencies_properties = dependency_type.properties();
    let mut units = HashSet::new();

    let mut dependency = Dependency::new(unit_name);
    //writeln!(out, "{}", unit_name).unwrap();
    reteive_dependencies(
        &mut dependency,
        unit_object_path,
        dependencies_properties,
        &connection,
        &mut units,
    )?;

    if plain {
        let mut all_children = BTreeSet::new();
        flatit(&dependency, &mut all_children);
        dependency.children.clear();
        dependency.children.append(&mut all_children);
    }

    Ok(dependency)
}

fn flatit(parent: &Dependency, all_children: &mut BTreeSet<Dependency>) {
    for child in parent.children.iter() {
        flatit(child, all_children);
        all_children.insert(child.partial_clone());
    }
}

fn reteive_dependencies(
    dependency: &mut Dependency,
    unit_object_path: &str,
    dependencies_properties: &[&str],
    connection: &Connection,
    units: &mut HashSet<String>,
) -> Result<(), SystemdErrors> {
    let map = fetch_unit_all_properties(connection, unit_object_path)?;

    dependency.state = map.get("ActiveState").into();
    let mut set = BTreeSet::new();
    //let mut set = BTreeSet::new();
    for property_key in dependencies_properties {
        let value = map.get(*property_key);
        let Some(value) = value else {
            warn!("property key {:?} does't exist", property_key);
            continue;
        };

        let array: &Array = value.try_into()?;

        for sv in array.iter() {
            let unit_name: &str = sv.try_into()?;

            if units.contains(unit_name) {
                continue;
            }

            set.insert(unit_name);
            units.insert(unit_name.to_string());
        }
    }

    for child_name in set {
        let objet_path = unit_dbus_path_from_name(child_name);

        let mut child_depency = Dependency::new(child_name);

        reteive_dependencies(
            &mut child_depency,
            &objet_path,
            dependencies_properties,
            connection,
            units,
        )?;

        dependency.children.insert(child_depency);
    }

    //units.remove(parent_unit_name);
    Ok(())
}

fn fetch_unit_all_properties(
    connection: &Connection,
    path: &str,
) -> Result<HashMap<String, OwnedValue>, SystemdErrors> {
    let proxy = Proxy::new(
        connection,
        DESTINATION_SYSTEMD,
        path,
        "org.freedesktop.DBus.Properties",
    )?;

    let all_properties: HashMap<String, OwnedValue> =
        match proxy.call("GetAll", &(INTERFACE_SYSTEMD_UNIT)) {
            Ok(m) => m,
            Err(e) => {
                warn!("{:#?}", e);
                return Err(e.into());
            }
        };

    Ok(all_properties)
}

pub(super) fn unit_dbus_path_from_name(name: &str) -> String {
    let converted = bus_label_escape(name);
    const PREFIX: &str = "/org/freedesktop/systemd1/unit/";

    let mut out = String::with_capacity(PREFIX.len() + converted.len());
    out.push_str(PREFIX);
    out.push_str(&converted);
    out
}

fn bus_label_escape(name: &str) -> String {
    /* Escapes all chars that D-Bus' object path cannot deal
     * with. Can be reversed with bus_path_unescape(). We special
     * case the empty string. */

    if name.is_empty() {
        return String::from("_");
    }

    let mut r = String::with_capacity(name.len() * 3 + 1);

    /* Escape everything that is not a-zA-Z0-9. We also escape 0-9 if it's the first character */
    for (i, c) in name.bytes().enumerate() {
        if !c.is_ascii_alphabetic() || i != 0 && c.is_ascii_digit() {
            r.push('_');
            r.push(hexchar(c >> 4));
            r.push(hexchar(c));
        } else {
            r.push(c as char);
        }
    }

    r
}

fn hexchar(x: u8) -> char {
    const TABLE: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];

    TABLE[(x & 15) as usize]
}

pub fn get_unit_active_state(
    dbus_level: UnitDBusLevel,
    unit_path: &str,
) -> Result<ActiveState, SystemdErrors> {
    let connection = get_connection(dbus_level)?;

    let proxy = Proxy::new(
        &connection,
        DESTINATION_SYSTEMD,
        unit_path,
        INTERFACE_SYSTEMD_UNIT,
    )?;

    let active_state: Str = proxy.get_property("ActiveState")?;

    Ok(ActiveState::from(active_state.as_str()))
}

#[derive(Deserialize, Type, PartialEq, Debug)]
pub(super) struct UnitProcessDeserialize {
    pub(super) path: String,
    pub(super) pid: u32,
    pub(super) name: String,
}

pub fn retreive_unit_processes(
    dbus_level: UnitDBusLevel,
    unit_name: &str,
) -> Result<Vec<UnitProcessDeserialize>, SystemdErrors> {
    let connection = get_connection(dbus_level)?;

    let message = connection.call_method(
        Some(DESTINATION_SYSTEMD),
        PATH_SYSTEMD,
        Some(INTERFACE_SYSTEMD_MANAGER),
        METHOD_GET_UNIT_PROCESSES,
        &(unit_name),
    )?;

    let unit_processes: Vec<UnitProcessDeserialize> = message.body().deserialize()?;

    Ok(unit_processes)
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;

    use super::*;

    pub const TEST_SERVICE: &str = "tiny_daemon.service";

    fn init() {
        let _ = env_logger::builder()
            .target(env_logger::Target::Stdout)
            .filter_level(log::LevelFilter::Debug)
            .is_test(true)
            .try_init();
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn stop_service_test() -> Result<(), SystemdErrors> {
        stop_unit(UnitDBusLevel::System, TEST_SERVICE, StartStopMode::Fail)?;
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_get_unit_file_state() {
        init();
        let file1: &str = TEST_SERVICE;

        let status = get_unit_file_state(UnitDBusLevel::System, file1);
        debug!("Status: {:?}", status);
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_list_unit_files() -> Result<(), SystemdErrors> {
        let units = list_unit_files(
            &get_connection(UnitDBusLevel::System)?,
            UnitDBusLevel::System,
        )?;

        let serv = units.iter().find(|ud| ud.full_name == TEST_SERVICE);

        debug!("{:#?}", serv);
        Ok(())
    }
    /*
    #[ignore = "need a connection to a service"]
    #[test]
    fn test_list_units() -> Result<(), SystemdErrors> {
        let units = list_units_description(
            &get_connection(UnitDBusLevel::System)?,
            UnitDBusLevel::System,
        )?;

        let serv = units.get(TEST_SERVICE);
        debug!("{:#?}", serv);
        Ok(())
    } */

    #[ignore = "need a connection to a service"]
    #[test]
    pub fn test_get_unit_path() -> Result<(), SystemdErrors> {
        let unit_file: &str = "tiny_daemon.service";

        let connection = get_connection(UnitDBusLevel::System)?;

        let message = connection.call_method(
            Some(DESTINATION_SYSTEMD),
            PATH_SYSTEMD,
            Some(INTERFACE_SYSTEMD_MANAGER),
            "GetUnit",
            &(unit_file),
        )?;

        println!("message {:?}", message);

        let body = message.body();

        let z: zvariant::ObjectPath = body.deserialize()?;
        //let z :String = body.deserialize()?;

        println!("obj {:?}", z.as_str());

        /*         let body = message.body();

        let des = body.deserialize();

        println!("{:#?}", des); */
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    pub fn test_fetch_system_unit_info() -> Result<(), SystemdErrors> {
        init();

        let btree_map = fetch_system_unit_info(
            UnitDBusLevel::System,
            "/org/freedesktop/systemd1/unit/tiny_5fdaemon_2eservice",
            UnitType::Service,
        )?;

        debug!("ALL PARAM: {:#?}", btree_map);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_enable_unit_files() -> Result<(), SystemdErrors> {
        init();
        let _res = enable_unit_files(
            UnitDBusLevel::System,
            &[TEST_SERVICE],
            DisEnableFlags::SD_SYSTEMD_UNIT_FORCE,
        )?;

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_disable_unit_files() -> Result<(), SystemdErrors> {
        init();
        let _res = disable_unit_files(
            UnitDBusLevel::System,
            &[TEST_SERVICE],
            DisEnableFlags::empty(),
        )?;

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_info() -> Result<(), SystemdErrors> {
        init();

        let path = unit_dbus_path_from_name(TEST_SERVICE);

        println!("unit {} Path {}", TEST_SERVICE, path);
        let map = fetch_system_unit_info(UnitDBusLevel::System, &path, UnitType::Service)?;

        println!("{:#?}", map);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_system_info() -> Result<(), SystemdErrors> {
        init();

        let map = fetch_system_info(UnitDBusLevel::System)?;

        info!("{:#?}", map);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit() -> Result<(), SystemdErrors> {
        init();

        let unit = fetch_unit(UnitDBusLevel::System, TEST_SERVICE)?;

        info!("{:#?}", unit);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit_user_session() -> Result<(), SystemdErrors> {
        init();

        let unit = fetch_unit(UnitDBusLevel::UserSession, TEST_SERVICE)?;

        info!("{:#?}", unit);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit_wrong_bus() -> Result<(), SystemdErrors> {
        init();

        let unit = fetch_unit(UnitDBusLevel::UserSession, TEST_SERVICE)?;

        info!("{}", unit.debug());
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit_dependencies() -> Result<(), SystemdErrors> {
        init();

        let path = unit_dbus_path_from_name(TEST_SERVICE);
        let res = unit_get_dependencies(
            UnitDBusLevel::System,
            TEST_SERVICE,
            &path,
            DependencyType::Forward,
            false,
        );

        info!("{:#?}", res.unwrap());
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit_reverse_dependencies() -> Result<(), SystemdErrors> {
        init();

        let path = unit_dbus_path_from_name(TEST_SERVICE);
        let res = unit_get_dependencies(
            UnitDBusLevel::System,
            TEST_SERVICE,
            &path,
            DependencyType::Reverse,
            false,
        );

        info!("{:#?}", res);
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_fetch_unit_fail_wrong_name() -> Result<(), SystemdErrors> {
        init();

        let fake = format!("{TEST_SERVICE}_fake");
        match fetch_unit(UnitDBusLevel::System, &fake) {
            Ok(_) => todo!(),
            Err(e) => {
                warn!("{:?}", e);
                if let SystemdErrors::ZNoSuchUnit(_method, _message) = e {
                    Ok(())
                } else {
                    Err(SystemdErrors::Custom("Wrong expected Error".to_owned()))
                }
            }
        }
    }

    #[test]
    fn test_name_convertion() {
        let tests = [
            ("tiny_daemon.service", "tiny_5fdaemon_2eservice"),
            ("-.mount", "_2d_2emount"),
            //("sys-devices-pci0000:00-0000:00:1d.0-0000:3d:00.0-nvme-nvme0-nvme0n1-nvme0n1p1.device", "sys_2ddevices_2dpci0000_3a00_2d0000_3a00_3a1d_2e0_2d0000_3a3d_3a00_2e0_2dnvme_2dnvme0_2dnvme0n1_2dnvme0n1p1_2edevice"),
            ("1first", "_31first"),
        ];

        for (origin, expected) in tests {
            let convertion = bus_label_escape(origin);
            assert_eq!(convertion, expected);
        }
    }

    #[derive(Deserialize, Type, PartialEq, Debug)]
    struct TestTruct {
        a: String,
        b: u32,
        c: String,
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_get_unit_processes() -> Result<(), SystemdErrors> {
        let unit_file: &str = "system.slice";

        let list = retreive_unit_processes(UnitDBusLevel::System, unit_file)?;

        for up in list {
            println!("{:#?}", up)
        }

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_get_unit_active_state() -> Result<(), SystemdErrors> {
        let unit_object = unit_dbus_path_from_name(TEST_SERVICE);

        println!("path : {unit_object}");
        let state = get_unit_active_state(UnitDBusLevel::System, &unit_object)?;

        println!("state of {TEST_SERVICE} is {:?}", state);

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[tokio::test]
    async fn test_get_list() -> Result<(), SystemdErrors> {
        let connection = get_connection_async(UnitDBusLevel::System).await?;
        let conn = Arc::new(connection);

        let connection2 = get_connection_async(UnitDBusLevel::UserSession).await?;
        let conn2 = Arc::new(connection2);

        use std::time::Instant;
        let now = Instant::now();
        let t1 = tokio::spawn(list_units_description_conn_async(
            conn.clone(),
            UnitDBusLevel::System,
        ));
        let t2 = tokio::spawn(list_unit_files_async(conn, UnitDBusLevel::System));
        let t3 = tokio::spawn(list_units_description_conn_async(
            conn2.clone(),
            UnitDBusLevel::UserSession,
        ));
        let t4 = tokio::spawn(list_unit_files_async(conn2, UnitDBusLevel::UserSession));

        let _asdf = tokio::join!(t1, t2, t3, t4);

        let elapsed = now.elapsed();
        println!("Elapsed: {:.2?}", elapsed);
        /*        let a = asdf.0;
        let b = asdf.1;

        println!("{:?}", a);
        println!("{:?}", b); */

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[tokio::test]
    async fn test_get_list2() -> Result<(), SystemdErrors> {
        let connection = get_connection_async(UnitDBusLevel::System).await?;
        let conn = Arc::new(connection);

        let connection2 = get_connection_async(UnitDBusLevel::UserSession).await?;
        let conn2 = Arc::new(connection2);

        use std::time::Instant;
        let now = Instant::now();
        let t1 = list_units_description_conn_async(conn.clone(), UnitDBusLevel::System);
        let t2 = list_unit_files_async(conn, UnitDBusLevel::System);
        let t3 = list_units_description_conn_async(conn2.clone(), UnitDBusLevel::UserSession);
        let t4 = list_unit_files_async(conn2, UnitDBusLevel::UserSession);

        let joined_result = tokio::join!(t1, t2, t3, t4);

        let elapsed = now.elapsed();
        println!("Elapsed: {:.2?}", elapsed);

        let r1 = joined_result.0.unwrap();
        let r2 = joined_result.1.unwrap();
        let r3 = joined_result.2.unwrap();
        let r4 = joined_result.3.unwrap();

        println!("System unit description size {}", r1.len());
        println!("System unit file size {}", r2.len());
        println!("Session unit description size {}", r3.len());
        println!("Session unit file size {}", r4.len());

        //check system collision
        for (key, _val) in r1 {
            if r3.contains_key(&key) {
                println!("collision description on key {}", key);
            }
        }

        /*         let a = asdf.0;
               let b = asdf.1;

               println!("{:?}", a);
               println!("{:?}", b);
        */
        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_get_properties() -> Result<(), SystemdErrors> {
        init();
        let connection = get_connection(UnitDBusLevel::System)?;

        let object_path = unit_dbus_path_from_name(TEST_SERVICE);
        debug!("Unit path: {object_path}");
        let properties_proxy: zbus::blocking::fdo::PropertiesProxy =
            fdo::PropertiesProxy::builder(&connection)
                .destination(DESTINATION_SYSTEMD)?
                .path(object_path)?
                // .interface(INTERFACE_SYSTEMD_UNIT)?
                //  .interface(UnitType::Service.interface())?
                .build()?;

        let unit_type = UnitType::Service;
        let unit_interface = unit_type.interface();

        //let unit_interface_name = InterfaceName::try_from(INTERFACE_SYSTEMD_UNIT).unwrap();
        let unit_interface_name = InterfaceName::try_from(INTERFACE_SYSTEMD_UNIT).unwrap();

        let mut unit_properties: HashMap<String, OwnedValue> =
            properties_proxy.get_all(unit_interface_name)?;

        let interface_name = InterfaceName::try_from(unit_interface).unwrap();

        let properties: HashMap<String, OwnedValue> = properties_proxy.get_all(interface_name)?;

        info!("Properties size {}", properties.len());

        info!("Unit Properties size {}", unit_properties.len());

        for k in properties.into_keys() {
            unit_properties.remove(&k);
        }

        info!("Unit Properties size {}", unit_properties.len());

        /*      for (k, v) in unit_properties {
            info!("{k} {:?}", v);
        } */

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_kill_unit() -> Result<(), SystemdErrors> {
        init();
        let unit_name: &str = TEST_SERVICE;

        kill_unit(UnitDBusLevel::System, unit_name, KillWho::Main, 1)
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_queue_signal_unit() -> Result<(), SystemdErrors> {
        init();
        let unit_name: &str = TEST_SERVICE;
        let val: i32 = libc::SIGRTMIN();
        let val2 = libc::SIGRTMAX();

        println!("{val} {val2}");

        queue_signal_unit(UnitDBusLevel::System, unit_name, KillWho::Main, 9, 0)
    }

    #[ignore = "need a connection to a service"]
    #[test]
    pub(super) fn test_unit_clean() -> Result<(), SystemdErrors> {
        let handle_answer = |_method: &str, _return_message: &Message| {
            info!("Clean Unit SUCCESS");

            Ok(())
        };
        let path = unit_dbus_path_from_name(TEST_SERVICE);
        let what = ["logs"];

        send_unit_message(
            UnitDBusLevel::System,
            "Clean",
            &(&what),
            handle_answer,
            &path,
        )
    }

    fn send_unit_message<T, U>(
        level: UnitDBusLevel,
        method: &str,
        body: &T,
        handler: impl Fn(&str, &Message) -> Result<U, SystemdErrors>,
        path: &str,
    ) -> Result<U, SystemdErrors>
    where
        T: serde::ser::Serialize + DynamicType,
        U: std::fmt::Debug,
    {
        let message = Message::method_call(path, method)?
            .with_flags(Flags::AllowInteractiveAuth)?
            .destination(DESTINATION_SYSTEMD)?
            .interface(INTERFACE_SYSTEMD_UNIT)?
            .build(body)?;

        let connection = get_connection(level)?;

        connection.send(&message)?;

        let message_it = MessageIterator::from(connection);

        for message_res in message_it {
            debug!("Message response {:?}", message_res);
            let return_message = message_res?;

            match return_message.message_type() {
                zbus::message::Type::MethodReturn => {
                    info!("{method} Response");
                    let result = handler(method, &return_message);
                    return result;
                }
                zbus::message::Type::MethodCall => {
                    warn!("Not supposed to happen: {:?}", return_message);
                    break;
                }
                zbus::message::Type::Error => {
                    let error = zbus::Error::from(return_message);
                    {
                        match error {
                            zbus::Error::MethodError(
                                ref owned_error_name,
                                ref details,
                                ref message,
                            ) => {
                                warn!(
                                    "Method error: {}\nDetails: {}\n{:?}",
                                    owned_error_name.as_str(),
                                    details.as_ref().map(|s| s.as_str()).unwrap_or_default(),
                                    message
                                )
                            }
                            _ => warn!("Bus error: {:?}", error),
                        }
                    }
                    return Err(SystemdErrors::from(error));
                }
                zbus::message::Type::Signal => {
                    info!("Signal: {:?}", return_message);
                    continue;
                }
            }
        }

        let msg = format!("{:?} ????, response supposed to be Unreachable", method);
        warn!("{}", msg);
        Err(SystemdErrors::Malformed(
            msg,
            "sequences of messages".to_owned(),
        ))
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_mask_unit_file() -> Result<(), SystemdErrors> {
        init();
        let unit_name: &str = TEST_SERVICE;

        mask_unit_files(UnitDBusLevel::System, &[unit_name], false, false)?;

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_mask_unit_file2() -> Result<(), SystemdErrors> {
        init();
        let unit_file_name: &str = "/etc/systemd/system/tiny_daemon.service";

        mask_unit_files(UnitDBusLevel::System, &[unit_file_name], false, false)?;

        Ok(())
    }

    #[ignore = "need a connection to a service"]
    #[test]
    fn test_unmask_unit_file() -> Result<(), SystemdErrors> {
        init();
        let unit_name: &str = TEST_SERVICE;

        unmask_unit_files(UnitDBusLevel::System, &[unit_name], false)?;

        Ok(())
    }
}
