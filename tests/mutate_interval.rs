use test_log::test;

use bevy::{prelude::*, state::app::StatesPlugin};
use bevy_replicon::{
    client::server_mutate_ticks::ServerMutateTicks,
    prelude::*,
    server::server_tick::ServerTick,
    test_app::{ServerTestAppExt, TestClientEntity},
};
use serde::{Deserialize, Serialize};

#[test]
fn mutations_wait_for_due_tick() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false)))
        .id();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();
    server_app.exchange_with_client(&mut client_app);

    skip_next_tick(&mut server_app, &client_app);

    server_app
        .world_mut()
        .get_mut::<BoolComponent>(server_entity)
        .unwrap()
        .0 = true;

    server_app.update();
    let mut messages = server_app.world_mut().resource_mut::<ServerMessages>();
    assert_eq!(
        messages.drain_sent().len(),
        0,
        "mutations shouldn't be sent outside of the interval"
    );

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut components = client_app.world_mut().query::<&BoolComponent>();
    let component = components.single(client_app.world()).unwrap();
    assert!(component.0, "mutation should be sent on the due tick");
}

#[test]
fn insertion_waits_for_due_tick() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .replicate::<UnitComponent>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false)))
        .id();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();
    server_app.exchange_with_client(&mut client_app);

    skip_next_tick(&mut server_app, &client_app);

    let mut entity = server_app.world_mut().entity_mut(server_entity);
    entity.get_mut::<BoolComponent>().unwrap().0 = true;
    entity.insert(UnitComponent);

    server_app.update();
    let mut messages = server_app.world_mut().resource_mut::<ServerMessages>();
    assert_eq!(
        messages.drain_sent().len(),
        0,
        "insertions shouldn't be sent outside of the interval"
    );

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut components = client_app
        .world_mut()
        .query::<(&BoolComponent, Has<UnitComponent>)>();
    let (component, inserted) = components.single(client_app.world()).unwrap();
    assert!(inserted);
    assert!(component.0);
}

#[test]
fn new_entity_waits_for_due_tick() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    skip_next_tick(&mut server_app, &client_app);

    server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(true)));

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut components = client_app.world_mut().query::<&BoolComponent>();
    assert_eq!(
        components.iter(client_app.world()).len(),
        0,
        "new entities shouldn't be sent outside of the interval"
    );

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let component = components.single(client_app.world()).unwrap();
    assert!(component.0);
}

#[test]
fn removal_outside_interval() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .replicate::<UnitComponent>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false), UnitComponent))
        .id();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();
    server_app.exchange_with_client(&mut client_app);

    skip_next_tick(&mut server_app, &client_app);

    let mut entity = server_app.world_mut().entity_mut(server_entity);
    entity.get_mut::<BoolComponent>().unwrap().0 = true;
    entity.remove::<UnitComponent>();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut components = client_app
        .world_mut()
        .query::<(&BoolComponent, Has<UnitComponent>)>();
    let (component, has_unit) = components.single(client_app.world()).unwrap();
    assert!(!has_unit, "removals should be sent on every tick");
    assert!(
        component.0,
        "mutations of an entity with a removal should be sent with it"
    );
}

#[test]
fn despawn_outside_interval() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false)))
        .id();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();
    server_app.exchange_with_client(&mut client_app);

    skip_next_tick(&mut server_app, &client_app);

    server_app.world_mut().despawn(server_entity);

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut components = client_app.world_mut().query::<&BoolComponent>();
    assert_eq!(
        components.iter(client_app.world()).len(),
        0,
        "despawns should be sent on every tick"
    );
}

#[test]
fn no_empty_messages_outside_interval() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin {
                track_mutate_messages: true,
                ..ServerPlugin::new(PostUpdate)
            }),
        ))
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let tick = skip_next_tick(&mut server_app, &client_app);

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mutate_ticks = client_app.world().resource::<ServerMutateTicks>();
    assert!(
        !mutate_ticks.contains(RepliconTick::new(tick + 1)),
        "skipped tick shouldn't be confirmed"
    );

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mutate_ticks = client_app.world().resource::<ServerMutateTicks>();
    assert!(mutate_ticks.contains(RepliconTick::new(tick + 2)));
}

/// Gives the client an interval of 2 ticks that leaves out the next tick.
///
/// Returns the current tick.
fn skip_next_tick(server_app: &mut App, client_app: &App) -> u32 {
    let tick = server_app.world().resource::<ServerTick>().get();
    let client = **client_app.world().resource::<TestClientEntity>();
    let interval = MutateInterval::new(2, tick);
    assert!(!interval.is_due(RepliconTick::new(tick + 1)));
    server_app.world_mut().entity_mut(client).insert(interval);
    tick
}

#[derive(Component, Deserialize, Serialize)]
struct BoolComponent(bool);

#[derive(Component, Deserialize, Serialize)]
struct UnitComponent;
