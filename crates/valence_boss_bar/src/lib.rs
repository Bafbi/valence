#![doc = include_str!("../README.md")]
#![allow(clippy::type_complexity)]
#![deny(
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links,
    rustdoc::missing_crate_level_docs,
    rustdoc::invalid_codeblock_attributes,
    rustdoc::invalid_rust_codeblocks,
    rustdoc::bare_urls,
    rustdoc::invalid_html_tags
)]
#![warn(
    trivial_casts,
    trivial_numeric_casts,
    unused_lifetimes,
    unused_import_braces,
    unreachable_pub,
    clippy::dbg_macro
)]

use std::borrow::Cow;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use packet::{BossBarAction, BossBarS2c, ToPacketAction};
use valence_client::{Client, FlushPacketsSet, VisibleEntityLayers, ViewDistance, OldViewDistance, OldVisibleEntityLayers, UpdateClientsSet};
use valence_core::chunk_pos::ChunkPos;
use valence_core::{block_pos::BlockPos, chunk_pos::ChunkView};
use valence_core::despawn::Despawned;
use valence_core::protocol::encode::WritePacket;
use valence_core::uuid::UniqueId;

mod components;
pub use components::*;
use valence_entity::{EntityLayerId, Position, OldPosition};
use valence_layer::{EntityLayer, Layer};

pub mod packet;

pub struct BossBarPlugin;

impl Plugin for BossBarPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_systems(
            PostUpdate,
            (
                update_boss_bar::<BossBarTitle>,
                update_boss_bar::<BossBarHealth>,
                update_boss_bar::<BossBarStyle>,
                update_boss_bar::<BossBarFlags>,
                update_boss_bar_view_and_layer.in_set(UpdateClientsSet),
                boss_bar_despawn,
            )
                .before(FlushPacketsSet),
        );
    }
}

fn update_boss_bar<'a, T: Component + ToPacketAction<'a> + 'a>(
    boss_bars_query: Query<
        (&UniqueId, &T, &EntityLayerId, Option<&Position>),
        Changed<T>,
    >,
    mut entity_layers_query: Query<&mut EntityLayer>,
) {
    for (id, part, entity_layer_id, pos) in boss_bars_query.iter() {
        if let Ok(mut entity_layer) = entity_layers_query.get_mut(entity_layer_id.0) {
            let packet = BossBarS2c {
                id: id.0,
                action: part.to_packet_action(),
            };
            if let Some(pos) = pos {
                entity_layer.view_writer(pos.to_chunk_pos()).write_packet(&packet);
            }
            else {
                entity_layer.write_packet(&packet);
            }
        }
    }
}

/// System that sends a bossbar add/remove packet to all viewers of a boss bar
/// that just have been added/removed.
fn update_boss_bar_view_and_layer(
    mut clients_query: Query<
        (
            Entity,
            &mut Client,
            Ref<VisibleEntityLayers>,
            &mut OldVisibleEntityLayers,
            &Position,
            &OldPosition,
            &ViewDistance,
            &OldViewDistance,
        ),
        Or<(
            Changed<VisibleEntityLayers>,
            Changed<Position>,
            Changed<ViewDistance>,
        )>,
    >,
    mut boss_bars_query: Query<
        (
            &UniqueId,
            &BossBarTitle<'static>,
            &BossBarHealth,
            &BossBarStyle,
            &BossBarFlags,
            &EntityLayerId,
        ),
    >,
    entity_layers_query: Query<&EntityLayer>,
) {
    // for (entity, client, visible_entity_layers, mut old_visible_entity_layers, pos, old_pos, view_distance, old_view_distance) in clients_query.iter_mut() {
    //     let view = ChunkView::new(ChunkPos::from_pos(pos.0), view_distance.get());
    //     let old_view = ChunkView::new(ChunkPos::from_pos(old_pos.get()), old_view_distance.get());

    //     if visible_entity_layers.is_changed() {
    //         // Remove all bossbar layers that are no longer visible in the old view.
    //         for &layer in old_visible_entity_layers.get()
    //             .difference(&visible_entity_layers.0)
    //         {
                
    //         }

    //         remove_buf.send_and_clear(&mut *client);

    //         // Load all entity layers that are newly visible in the old view.
    //         for &layer in visible_entity_layers
    //             .0
    //             .difference(&old_visible_entity_layers.0)
    //         {
    //             if let Ok(layer) = entity_layers.get(layer) {
    //                 for pos in old_view.iter() {
    //                     for entity in layer.entities_at(pos) {
    //                         if self_entity != entity {
    //                             if let Ok((init, pos)) = entity_init.get(entity) {
    //                                 init.write_init_packets(pos.get(), &mut *client);
    //                             }
    //                         }
    //                     }
    //                 }
    //             }
    //         }
    //     }

    // }

    // for (id, title, health, style, flags, mut boss_bar_viewers) in boss_bars.iter_mut() {
    //     let old_viewers = &boss_bar_viewers.old_viewers;
    //     let current_viewers = &boss_bar_viewers.viewers;

    //     for &added_viewer in current_viewers.difference(old_viewers) {
    //         if let Ok(mut client) = clients.get_mut(added_viewer) {
    //             client.write_packet(&BossBarS2c {
    //                 id: id.0,
    //                 action: BossBarAction::Add {
    //                     title: Cow::Borrowed(&title.0),
    //                     health: health.0,
    //                     color: style.color,
    //                     division: style.division,
    //                     flags: *flags,
    //                 },
    //             });
    //         }
    //     }

    //     for &removed_viewer in old_viewers.difference(current_viewers) {
    //         if let Ok(mut client) = clients.get_mut(removed_viewer) {
    //             client.write_packet(&BossBarS2c {
    //                 id: id.0,
    //                 action: BossBarAction::Remove,
    //             });
    //         }
    //     }

    //     boss_bar_viewers.old_viewers = boss_bar_viewers.viewers.clone();
    // }
}

/// System that sends a bossbar remove packet to all viewers of a boss bar that
/// has been despawned.
fn boss_bar_despawn(
    mut boss_bars: Query<(&UniqueId, &BossBarViewers), Added<Despawned>>,
    mut clients: Query<&mut Client>,
) {
    for (id, viewers) in boss_bars.iter_mut() {
        for viewer in viewers.viewers.iter() {
            if let Ok(mut client) = clients.get_mut(*viewer) {
                client.write_packet(&BossBarS2c {
                    id: id.0,
                    action: BossBarAction::Remove,
                });
            }
        }
    }
}

/// System that removes a client from the viewers of its boss bars when it
/// disconnects.
fn client_disconnection(
    disconnected_clients: Query<Entity, (With<Client>, Added<Despawned>)>,
    mut boss_bars_viewers: Query<&mut BossBarViewers>,
) {
    for entity in disconnected_clients.iter() {
        for mut boss_bar_viewers in boss_bars_viewers.iter_mut() {
            boss_bar_viewers.viewers.remove(&entity);
        }
    }
}
