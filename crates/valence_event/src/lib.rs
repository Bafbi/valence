use bevy_app::prelude::*;
use bevy_ecs::{prelude::Event, system::Local};
use state_event::{EventsWithState, EventWithStateReader, StateEventWriter};

pub mod state_event;

pub struct EventPlugin;
