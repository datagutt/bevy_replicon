use test_log::test;

use bevy::{prelude::*, state::app::StatesPlugin};
use bevy_replicon::{
    prelude::*,
    server::visibility::{client_visibility::ClientVisibility, registry::FilterRegistry},
    shared::replication::registry::ReplicationRegistry,
    test_app::{ServerTestAppExt, TestClientEntity},
};
use serde::{Deserialize, Serialize};

#[test]
fn several_clients() {
    let mut server_app = App::new();
    let mut client_app1 = App::new();
    let mut client_app2 = App::new();
    for app in [&mut server_app, &mut client_app1, &mut client_app2] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .add_visibility_filter::<EntityVisibility>()
        .finish();
    }

    server_app.connect_client(&mut client_app1);
    server_app.connect_client(&mut client_app2);

    let client1 = **client_app1.world().resource::<TestClientEntity>();
    server_app
        .world_mut()
        .entity_mut(client1)
        .insert(EntityVisibility);

    server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false), EntityVisibility));
    server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false)));

    server_app.update();
    for client_app in [&mut client_app1, &mut client_app2] {
        server_app.exchange_with_client(client_app);
        client_app.update();
    }

    let mut replicated = client_app1.world_mut().query::<&BoolComponent>();
    assert_eq!(replicated.iter(client_app1.world()).len(), 2);

    let mut replicated = client_app2.world_mut().query::<&BoolComponent>();
    assert_eq!(
        replicated.iter(client_app2.world()).len(),
        1,
        "the second client shouldn't see the filtered entity"
    );
}

#[test]
fn reused_slot() {
    let mut server_app = App::new();
    let mut client_app1 = App::new();
    let mut client_app2 = App::new();
    for app in [&mut server_app, &mut client_app1, &mut client_app2] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .finish();
    }

    let bit =
        server_app
            .world_mut()
            .resource_scope(|world, mut filter_registry: Mut<FilterRegistry>| {
                world.resource_scope(|world, mut registry: Mut<ReplicationRegistry>| {
                    filter_registry.register_scope::<Entity>(
                        world,
                        &mut registry,
                        ScopeLifetime::WhileVisible,
                    )
                })
            });

    server_app.connect_client(&mut client_app1);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false)))
        .id();

    let client1 = **client_app1.world().resource::<TestClientEntity>();
    server_app
        .world_mut()
        .get_mut::<ClientVisibility>(client1)
        .unwrap()
        .set(server_entity, bit, false);

    server_app.update();
    server_app.exchange_with_client(&mut client_app1);
    client_app1.update();

    let mut components = client_app1.world_mut().query::<&BoolComponent>();
    assert_eq!(components.iter(client_app1.world()).len(), 0);

    server_app.disconnect_client(&mut client_app1);

    // Takes the slot of the first client, which had the entity hidden.
    server_app.connect_client(&mut client_app2);

    server_app.update();
    server_app.exchange_with_client(&mut client_app2);
    client_app2.update();

    let mut components = client_app2.world_mut().query::<&BoolComponent>();
    assert_eq!(
        components.iter(client_app2.world()).len(),
        1,
        "a new client shouldn't inherit the visibility of the previous client in its slot"
    );
}

#[test]
fn visibility_gain_and_loss() {
    let mut server_app = App::new();
    let mut client_app = App::new();
    for app in [&mut server_app, &mut client_app] {
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
        ))
        .replicate::<BoolComponent>()
        .add_visibility_filter::<EntityVisibility>()
        .finish();
    }

    server_app.connect_client(&mut client_app);

    let server_entity = server_app
        .world_mut()
        .spawn((Replicated, BoolComponent(false), EntityVisibility))
        .id();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let mut replicated = client_app.world_mut().query::<&BoolComponent>();
    assert_eq!(replicated.iter(client_app.world()).len(), 0);

    let client = **client_app.world().resource::<TestClientEntity>();
    server_app
        .world_mut()
        .entity_mut(client)
        .insert(EntityVisibility);

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();
    server_app.exchange_with_client(&mut client_app);

    assert_eq!(replicated.iter(client_app.world()).len(), 1);

    // Changes reach the client only while the entity is visible.
    server_app
        .world_mut()
        .get_mut::<BoolComponent>(server_entity)
        .unwrap()
        .0 = true;
    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    let component = replicated.single(client_app.world()).unwrap();
    assert!(component.0);

    server_app
        .world_mut()
        .entity_mut(client)
        .remove::<EntityVisibility>();

    server_app.update();
    server_app.exchange_with_client(&mut client_app);
    client_app.update();

    assert_eq!(replicated.iter(client_app.world()).len(), 0);
}

#[derive(Component, Deserialize, Serialize)]
struct BoolComponent(bool);

#[derive(Component)]
#[component(immutable)]
struct EntityVisibility;

impl VisibilityFilter for EntityVisibility {
    type ClientComponent = Self;
    type Scope = Entity;

    fn is_visible(&self, _client: Entity, component: Option<&Self::ClientComponent>) -> bool {
        component.is_some()
    }
}
