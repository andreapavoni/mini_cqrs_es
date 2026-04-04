#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

use mini_cqrs_es::{
    Aggregate, Command, CqrsError, Event, EventConsumer, EventPayload, Query, Uuid,
};

#[path = "sqlite_store.rs"]
mod sqlite_store;
pub use sqlite_store::*;

// --- Room State ---

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RoomState {
    Free,
    Occupied { guest_name: String },
}

// --- Events ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HotelEvent {
    HotelInitialized { room_count: u32 },
    GuestCheckedIn { room_number: u32, guest_name: String },
    GuestCheckedOut { room_number: u32 },
}

impl fmt::Display for HotelEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotelEvent::HotelInitialized { .. } => write!(f, "HotelInitialized"),
            HotelEvent::GuestCheckedIn { .. } => write!(f, "GuestCheckedIn"),
            HotelEvent::GuestCheckedOut { .. } => write!(f, "GuestCheckedOut"),
        }
    }
}

impl EventPayload for HotelEvent {}

// --- Aggregate ---

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotelAggregate {
    id: Uuid,
    version: u64,
    pub rooms: HashMap<u32, RoomState>,
}

impl Default for HotelAggregate {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            version: 0,
            rooms: HashMap::new(),
        }
    }
}

impl Aggregate for HotelAggregate {
    type Event = HotelEvent;

    async fn apply(&mut self, event: &Self::Event) {
        match event {
            HotelEvent::HotelInitialized { room_count } => {
                self.rooms.clear();
                for i in 1..=*room_count {
                    self.rooms.insert(i, RoomState::Free);
                }
            }
            HotelEvent::GuestCheckedIn {
                room_number,
                guest_name,
            } => {
                self.rooms.insert(
                    *room_number,
                    RoomState::Occupied {
                        guest_name: guest_name.clone(),
                    },
                );
            }
            HotelEvent::GuestCheckedOut { room_number } => {
                self.rooms.insert(*room_number, RoomState::Free);
            }
        }
    }

    fn aggregate_id(&self) -> Uuid {
        self.id
    }

    fn set_aggregate_id(&mut self, id: Uuid) {
        self.id = id;
    }

    fn version(&self) -> u64 {
        self.version
    }

    fn set_version(&mut self, version: u64) {
        self.version = version;
    }
}

// --- Commands ---

pub struct CmdInitializeHotel {
    pub room_count: u32,
}

impl Command for CmdInitializeHotel {
    type Aggregate = HotelAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<HotelEvent>, CqrsError> {
        if !aggregate.rooms.is_empty() {
            return Err(CqrsError::Domain("Hotel already initialized".into()));
        }
        Ok(vec![HotelEvent::HotelInitialized {
            room_count: self.room_count,
        }])
    }
}

pub struct CmdCheckIn {
    pub room_number: u32,
    pub guest_name: String,
}

impl Command for CmdCheckIn {
    type Aggregate = HotelAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<HotelEvent>, CqrsError> {
        match aggregate.rooms.get(&self.room_number) {
            Some(RoomState::Occupied { .. }) => Err(CqrsError::Domain(format!(
                "Room {} is occupied",
                self.room_number
            ))),
            Some(RoomState::Free) => Ok(vec![HotelEvent::GuestCheckedIn {
                room_number: self.room_number,
                guest_name: self.guest_name.clone(),
            }]),
            None => Err(CqrsError::Domain(format!(
                "Room {} does not exist",
                self.room_number
            ))),
        }
    }
}

pub struct CmdCheckOut {
    pub room_number: u32,
}

impl Command for CmdCheckOut {
    type Aggregate = HotelAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<HotelEvent>, CqrsError> {
        match aggregate.rooms.get(&self.room_number) {
            Some(RoomState::Free) => Err(CqrsError::Domain(format!(
                "Room {} is already free",
                self.room_number
            ))),
            Some(RoomState::Occupied { .. }) => Ok(vec![HotelEvent::GuestCheckedOut {
                room_number: self.room_number,
            }]),
            None => Err(CqrsError::Domain(format!(
                "Room {} does not exist",
                self.room_number
            ))),
        }
    }
}

// --- Read Model ---

#[derive(Clone, Debug, Default)]
pub struct HotelReadModel {
    pub rooms: HashMap<u32, RoomState>,
}

// --- Projection Consumer ---

#[derive(Clone)]
pub struct HotelProjectionConsumer {
    read_model: Arc<Mutex<HotelReadModel>>,
}

impl HotelProjectionConsumer {
    pub fn new(read_model: Arc<Mutex<HotelReadModel>>) -> Self {
        Self { read_model }
    }
}

impl EventConsumer for HotelProjectionConsumer {
    async fn process(&self, evt: &Event) {
        let Ok(event) = evt.get_payload::<HotelEvent>() else {
            return;
        };

        let mut model = self.read_model.lock().unwrap();
        match event {
            HotelEvent::HotelInitialized { room_count } => {
                model.rooms.clear();
                for i in 1..=room_count {
                    model.rooms.insert(i, RoomState::Free);
                }
            }
            HotelEvent::GuestCheckedIn {
                room_number,
                guest_name,
            } => {
                model
                    .rooms
                    .insert(room_number, RoomState::Occupied { guest_name });
            }
            HotelEvent::GuestCheckedOut { room_number } => {
                model.rooms.insert(room_number, RoomState::Free);
            }
        }
    }
}

// --- Query ---

#[derive(Clone)]
pub struct GetHotelStateQuery {
    read_model: Arc<Mutex<HotelReadModel>>,
}

impl GetHotelStateQuery {
    pub fn new(read_model: Arc<Mutex<HotelReadModel>>) -> Self {
        Self { read_model }
    }
}

impl Query for GetHotelStateQuery {
    type Output = HotelReadModel;

    async fn apply(&self) -> HotelReadModel {
        self.read_model.lock().unwrap().clone()
    }
}
