use mini_cqrs_es::{Aggregate, Command, CqrsError, EventPayload};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, str::FromStr};

// --- Room State ---
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HotelId(u32);

impl HotelId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

impl fmt::Display for HotelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for HotelId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RoomState {
    Free,
    Occupied { guest_name: String },
}

// --- Events ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HotelEvent {
    HotelInitialized {
        room_count: u32,
    },
    GuestCheckedIn {
        room_number: u32,
        guest_name: String,
    },
    GuestCheckedOut {
        room_number: u32,
    },
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

impl EventPayload for HotelEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::HotelInitialized { .. } => "HotelInitialized",
            Self::GuestCheckedIn { .. } => "GuestCheckedIn",
            Self::GuestCheckedOut { .. } => "GuestCheckedOut",
        }
    }
}

// --- Aggregate ---

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HotelAggregate {
    pub rooms: HashMap<u32, RoomState>,
}

impl Aggregate for HotelAggregate {
    type Id = HotelId;
    type Event = HotelEvent;

    fn aggregate_type() -> &'static str {
        "example.hotel"
    }

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
}

// --- Commands ---

pub struct CmdInitializeHotel {
    pub room_count: u32,
}

impl Command for CmdInitializeHotel {
    type Aggregate = HotelAggregate;

    async fn handle(&self, aggregate: &Self::Aggregate) -> Result<Vec<HotelEvent>, CqrsError> {
        if !aggregate.rooms.is_empty() {
            return Err(CqrsError::domain("Hotel already initialized"));
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
            Some(RoomState::Occupied { .. }) => Err(CqrsError::domain(format!(
                "Room {} is occupied",
                self.room_number
            ))),
            Some(RoomState::Free) => Ok(vec![HotelEvent::GuestCheckedIn {
                room_number: self.room_number,
                guest_name: self.guest_name.clone(),
            }]),
            None => Err(CqrsError::domain(format!(
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
            Some(RoomState::Free) => Err(CqrsError::domain(format!(
                "Room {} is already free",
                self.room_number
            ))),
            Some(RoomState::Occupied { .. }) => Ok(vec![HotelEvent::GuestCheckedOut {
                room_number: self.room_number,
            }]),
            None => Err(CqrsError::domain(format!(
                "Room {} does not exist",
                self.room_number
            ))),
        }
    }
}
