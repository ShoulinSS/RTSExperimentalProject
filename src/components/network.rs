use std::{net::{Ipv4Addr, Ipv6Addr, SocketAddr}, str::FromStr, sync::mpsc::channel};

use bevy::{audio::{PlaybackMode, Volume}, ecs::entity, log::tracing_subscriber::field::display::Messages, pbr::{ExtendedMaterial, NotShadowCaster}, prelude::*, reflect::enum_hash, render::MATHS_SHADER_HANDLE, ui::debug::print_ui_layout_tree, utils::hashbrown::{HashMap, HashSet}};
use bevy_quinnet::{client::{self, certificate::CertificateVerificationMode, client_connecting, connection::{ClientEndpointConfiguration, ConnectionEvent, ConnectionFailedEvent}, QuinnetClient}, server::{certificate::CertificateRetrievalMode, ConnectionLostEvent, QuinnetServer, ServerEndpointConfiguration}, shared::{channels::{ChannelType, ChannelsConfiguration}, ClientId}};
use bevy_rapier3d::{prelude::{CharacterLength, Collider, CollisionGroups, ComputedColliderShape, Group, KinematicCharacterController}, rapier::crossbeam::channel};
use oxidized_navigation_serializable::{Area, NavMesh, NavMeshAffector, NavMeshAreaType, NavMeshSettings};
use serde::{Deserialize, Serialize};

use crate::{GameStage, GameStages, GameState, HUMAN_RESOURCE_COLOR, MATERIALS_COLOR, PlayerData, components::{asset_manager::{AttackVisualisationAssets, BuildingsAssets, ChangeMaterial, CircleData, CircleHolder, InstancedMaterials, LOD, TeamMaterialExtension, TrailComponent, TrailEmmiterComponent, UnitAssets}, building::{CONSTRUCTION_PROGRESS_COLOR, ConstructionProgressBar, DeconstructableBuilding, DontTouch, HumanResourceStorageComponent, HumanResourcesDisplay, MaterialsDisplay, MaterialsProductionComponent, MaterialsStorageComponent, SettlementCaptureInProgress, SettlementCaptureProgressBar, Settlements, SwitchableBuilding, ToDeconstruct}, logistics::LOGISTIC_UNITS_SPEED, ui_manager::{HumanResourcesOverallAmountDisplay, MaterialsOverallAmountDisplay}, unit::{AsyncPathfindingTasks, AsyncTaskPools, AttackAnimationTypes, BusyEngineer, Covered, DisabledUnit, EngineerActions, ExplosionEvent, InTransport, InfantryTransport, LimitedNumber, MovingToCover, MovingToTransport, RemainsCount, START_ARMORED_SQUADS_AMOUNT, START_ARTILLERY_UNITS_COUNT, START_ENGINEERS_COUNT, START_REGULAR_SQUADS_AMOUNT, START_SHOCK_SQUADS_AMOUNT, SelectedUnit, SquadLeader, StoppedMoving, SuppliesConsumerComponent, TaskPoolTypes, UnitNeedsToBeUncovered, UnitRemains, UnstartedPathfindingTasksPool, async_path_find}}};

use super::{asset_manager::{generate_circle_segments, LineData, LineHolder}, building::{create_ring, AllSettlementsPlaced, ApartmentHouse, ArtilleryBundle, BuildingBlueprint, BuildingConstructionSite, BuildingsBundles, BuildingsList, CoverComponent, DeleteTemporaryObjects, EngineerBundle, IFVBundle, InfantryBarracksBundle, LogisticHubBundle, ProducableUnits, ProductionQueue, ProductionState, ResourceMinerBundle, SettlementComponent, SettlementObject, SoldierBundle, SuppliesProductionComponent, TankBundle, TemporaryObject, UnactivatedBlueprints, UnitBundles, VehicleFactoryBundle}, logistics::{create_curved_mesh, ResourceZone}, ui_manager::{Actions, ButtonAction, GameStartedEvent, ProductionStateChanged, UiButtonNodes}, unit::{self, Armies, ArmoredSquad, ArmyObject, ArtilleryNeedsToFire, ArtilleryUnit, AttackTypes, CompanyTypes, CombatComponent, DamageTypes, DeleteAfterStart, LimitedHashSet, NeedToMove, RegularSquad, SelectableUnit, SerializableArmyObject, ShockSquad, UnitComponent, UnitDeathEvent, UnitTypes, UnitsTileMap, ARMORED_SQUAD_SIZE, REGULAR_SQUAD_SIZE, SHOCK_SQUAD_SIZE, SPECIALISTS_PER_REGULAR_SQUAD, SPECIALISTS_PER_SHOCK_SQUAD, TILE_SIZE}};

#[derive(Resource)]
pub struct NetworkStatus(pub NetworkStatuses);

pub enum NetworkStatuses{
    SinglePlayer,
    Host,
    Client,
}

#[derive(Resource)]
pub struct InsertedConnectionData{
    pub ip: String,
    pub username: String,
}

#[derive(Resource)]
pub struct ClientList(pub HashMap<ClientId, (String, bool, bool)>);

#[derive(Resource)]
pub struct PlayerList(pub HashMap<i32, HashMap<ClientId, String>>);

#[derive(Resource)]
pub struct EntityMaps{
    pub server_to_client: HashMap<Entity, Entity>,
    pub client_to_server: HashMap<Entity, Entity>,
}

// #[derive(Event)]
// pub struct UnsentServerMessage(pub (u8, ServerMessage, i32));
// #[derive(Resource)]
// pub struct PendingServerMessages(pub Vec<(u8, ServerMessage, i32)>);

// #[derive(Event)]
// pub struct UnsentClientMessage(pub (u8, ClientMessage, i32));
// #[derive(Resource)]
// pub struct PendingClientMessages(pub Vec<(u8, ClientMessage, i32)>);

pub fn initialize_server_lobby(
    mut players: ResMut<PlayerList>,
    ip_buffer: Res<InsertedConnectionData>,
    mut player_data: ResMut<PlayerData>,
){
    players.0.insert(1, HashMap::new());
    players.0.get_mut(&1).unwrap().insert(0, ip_buffer.username.clone());

    players.0.insert(2, HashMap::new());

    player_data.team = 1;
}

const SERVER_HOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const LOCAL_BIND_IP: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
const SERVER_PORT: u16 = 6000;

pub fn start_listening_clients(mut server: ResMut<QuinnetServer>) {
    let _ = server.stop_endpoint();

    server
    .start_endpoint(
        ServerEndpointConfiguration::from_ip(LOCAL_BIND_IP, SERVER_PORT),
        CertificateRetrievalMode::GenerateSelfSigned {
            server_hostname: SERVER_HOST.to_string(),
        },
        ChannelsConfiguration::from_types(vec![
                ChannelType::Unreliable,            //0
                ChannelType::Unreliable,            //1
                ChannelType::Unreliable,            //2
                ChannelType::Unreliable,            //3
                ChannelType::Unreliable,            //4
                ChannelType::Unreliable,            //5
                ChannelType::Unreliable,            //6
                ChannelType::Unreliable,            //7
                ChannelType::Unreliable,            //8
                ChannelType::Unreliable,            //9
                ChannelType::Unreliable,            //10
                ChannelType::Unreliable,            //11
                ChannelType::Unreliable,            //12
                ChannelType::Unreliable,            //13
                ChannelType::Unreliable,            //14
                ChannelType::Unreliable,            //15
                ChannelType::Unreliable,            //16
                ChannelType::Unreliable,            //17
                ChannelType::Unreliable,            //18
                ChannelType::Unreliable,            //19
                ChannelType::Unreliable,            //20
                ChannelType::Unreliable,            //21
                ChannelType::Unreliable,            //22
                ChannelType::Unreliable,            //23
                ChannelType::Unreliable,            //24
                ChannelType::Unreliable,            //25
                ChannelType::Unreliable,            //26
                ChannelType::Unreliable,            //27
                ChannelType::Unreliable,            //28
                ChannelType::Unreliable,            //29

                ChannelType::UnorderedReliable,     //30
                ChannelType::UnorderedReliable,     //31
                ChannelType::UnorderedReliable,     //32
                ChannelType::UnorderedReliable,     //33
                ChannelType::UnorderedReliable,     //34
                ChannelType::UnorderedReliable,     //35
                ChannelType::UnorderedReliable,     //36
                ChannelType::UnorderedReliable,     //37
                ChannelType::UnorderedReliable,     //38
                ChannelType::UnorderedReliable,     //39
                ChannelType::UnorderedReliable,     //40
                ChannelType::UnorderedReliable,     //41
                ChannelType::UnorderedReliable,     //42
                ChannelType::UnorderedReliable,     //43
                ChannelType::UnorderedReliable,     //44
                ChannelType::UnorderedReliable,     //45
                ChannelType::UnorderedReliable,     //46
                ChannelType::UnorderedReliable,     //47
                ChannelType::UnorderedReliable,     //48
                ChannelType::UnorderedReliable,     //49
                ChannelType::UnorderedReliable,     //50
                ChannelType::UnorderedReliable,     //51
                ChannelType::UnorderedReliable,     //52
                ChannelType::UnorderedReliable,     //53
                ChannelType::UnorderedReliable,     //54
                ChannelType::UnorderedReliable,     //55
                ChannelType::UnorderedReliable,     //56
                ChannelType::UnorderedReliable,     //57
                ChannelType::UnorderedReliable,     //58
                ChannelType::UnorderedReliable,     //59

                ChannelType::OrderedReliable,       //60
                ChannelType::OrderedReliable,       //61
                ChannelType::OrderedReliable,       //62
                ChannelType::OrderedReliable,       //63
                ChannelType::OrderedReliable,       //64
                ChannelType::OrderedReliable,       //65
                ChannelType::OrderedReliable,       //66
                ChannelType::OrderedReliable,       //67
                ChannelType::OrderedReliable,       //68
                ChannelType::OrderedReliable,       //69
                ChannelType::OrderedReliable,       //70
                ChannelType::OrderedReliable,       //71
                ChannelType::OrderedReliable,       //72
                ChannelType::OrderedReliable,       //73
                ChannelType::OrderedReliable,       //74
                ChannelType::OrderedReliable,       //75
                ChannelType::OrderedReliable,       //76
                ChannelType::OrderedReliable,       //77
                ChannelType::OrderedReliable,       //78
                ChannelType::OrderedReliable,       //79
                ChannelType::OrderedReliable,       //80
                ChannelType::OrderedReliable,       //81
                ChannelType::OrderedReliable,       //82
                ChannelType::OrderedReliable,       //83
                ChannelType::OrderedReliable,       //84
                ChannelType::OrderedReliable,       //85
                ChannelType::OrderedReliable,       //86
                ChannelType::OrderedReliable,       //87
                ChannelType::OrderedReliable,       //88
                ChannelType::OrderedReliable,       //89
            ])
            .unwrap(),
    )
    .unwrap();
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ClientMessage{
    Connected{ name: String },
    SettlementPlacementRequest{
        settlement: SettlementObject,
        position: Vec3,
    },
    AllSettlementsPlaced,
    BuildingPlacementRequest{
        team: i32,
        name: String,
        position: Vec3,
        angle: f32,
        needed_buildpower: i32,
    },
    ArmySetupStageCompleted{
        army: SerializableArmyObject,
    },
    ClientArmyChanged{
        army: SerializableArmyObject,
    },
    ProductionStateChanged{
        team: i32,
        is_allowed: bool,
    },
    UnitPathInsertRequest{
        entity: Entity,
        path: Vec<Vec3>,
    },
    ArtilleryDesignationRequest{
        artillery_entity: Entity,
        target_position: Vec3,
    },
    CancelArtilleryFire{
        artillery_entity: Entity,
    },
    CoverAssignationRequest{
        units: Vec<Entity>,
        cover_entity: Entity,
        cover_position: Vec3,
    },
    UncoveringRequest{
        unit_entities: Vec<Entity>,
    },
    DeleteUnspecifiedEntityRequest{
        entity: Entity,
    },
    DeconstructionRequest{
        entity: Entity,
        team: i32,
        deconstruction_distance: f32,
    },
    DeconstructionCancelationRequest{
        entity: Entity,
        position: Vec3,
    },
    BuildingStateSwitchRequest{
        entity: Entity,
        state: bool,
    },
    ApartmentsRebuildingRequest{
        entity: Entity,
    },
    TransportAssignationRequest{
        units: Vec<Entity>,
        transport_entity: Entity,
        transport_position: Vec3,
    },
    TransportAssignationCancelRequest{
         units: Vec<Entity>,
    },
    DisembarkRequest{
        transports: Vec<Entity>,
    }
}

pub fn client_messages_handler(
    mut server: ResMut<QuinnetServer>,
    mut connection_lost_events: EventReader<ConnectionLostEvent>,
    mut clients: (
        ResMut<ClientList>,
        ResMut<PlayerList>,
    ),
    mut materials: (
        ResMut<Assets<StandardMaterial>>,
        ResMut<InstancedMaterials>,
        ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    ),
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut resources: (
        Res<BuildingsList>,
        Res<UiButtonNodes>,
        ResMut<UnitsTileMap>,
        Res<BuildingsAssets>,
    ),
    mut queries:(
        Query<(&mut ResourceZone, &Transform), With<ResourceZone>>,
        Query<&mut ArtilleryUnit>,
        Query<(Entity, &mut CoverComponent, &Transform, &CombatComponent), (With<CoverComponent>, Without<ResourceZone>)>,
        Query<(Entity, Option<&Covered>, &mut Transform), (Without<CoverComponent>, Without<ResourceZone>, Without<UnitComponent>, Without<InfantryTransport>)>,
        Query<&mut BuildingConstructionSite>,
        Query<&mut DeconstructableBuilding>,
        Query<&mut SwitchableBuilding>,
        Query<&mut SettlementComponent>,
        Query<(&mut MaterialsProductionComponent, &CombatComponent), Without<CoverComponent>>,
        Query<(&mut InfantryTransport, &Transform), (Without<ResourceZone>, Without<CoverComponent>)>,
        Query<&mut Transform, (Without<ResourceZone>, Without<CoverComponent>, With<UnitComponent>, Without<InfantryTransport>)>,
    ),
    mut moving_units_q: Query<&mut UnitComponent, With<UnitComponent>>,
    mut armies: ResMut<Armies>,
    mut production_queue: ResMut<ProductionQueue>,
    mut unactivated_blueprints: ResMut<UnactivatedBlueprints>,
    game_stage: Res<GameStage>,
    mut production_state: ResMut<ProductionState>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut event_writer: (
        EventWriter<ProductionStateChanged>,
        EventWriter<UnitNeedsToBeUncovered>,
    ),
){
    let endpoint = server.endpoint_mut();

    for disconnected_client in connection_lost_events.read() {
        clients.0.0.remove(&disconnected_client.id);

        for team in clients.1.0.iter_mut(){
            team.1.remove(&disconnected_client.id);
        }

        let mut serializable_player_list: Vec<(i32, Vec<(ClientId, String)>)> = Vec::new();

        for team in clients.1.0.iter() {
            let mut players_to_insert: Vec<(ClientId, String)> = Vec::new();

            for player in team.1.iter() {
                let id = player.0;
                let name = player.1;
                players_to_insert.push((*id, name.to_string()));
            }

            serializable_player_list.push((*team.0, players_to_insert));
        }

        let mut channel_id = 60;
        while channel_id <= 89 {
            if let Err(_) = endpoint
            .send_group_message_on(clients.0.0.keys(), channel_id,ServerMessage::PlayerQuit { player_list: serializable_player_list.clone() }){
                channel_id += 1;
            } else {
                break;
            }
        }
    }

    for client_id in endpoint.clients() {
        while let Some((_, message)) = endpoint.try_receive_message_from::<ClientMessage>(client_id)
        {
            match message {
                ClientMessage::Connected { name } => {
                    clients.0.0.insert(client_id, (name.clone(), false, false));

                    let mut team_to_insert = 0;
                    if let Some(team1) = clients.1.0.get(&1) {
                        if let Some(team2) = clients.1.0.get(&2) {
                            if team1.len() <= team2.len() {
                                team_to_insert = 1;
                            } else {
                                team_to_insert = 2;
                            }
                        }
                    }

                    if team_to_insert != 0 {
                        if let Some(team) = clients.1.0.get_mut(&team_to_insert) {
                            team.insert(client_id, name.clone());
                        }
                    }

                    let mut serializable_player_list: Vec<(i32, Vec<(ClientId, String)>)> = Vec::new();

                    for team in clients.1.0.iter() {
                        let mut players_to_insert: Vec<(ClientId, String)> = Vec::new();

                        for player in team.1.iter() {
                            let id = player.0;
                            let name = player.1;
                            players_to_insert.push((*id, name.to_string()));
                        }

                        serializable_player_list.push((*team.0, players_to_insert));
                    }

                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = endpoint
                        .send_group_message_on(clients.0.0.keys(), channel_id,ServerMessage::PlayerJoined { player_list: serializable_player_list.clone() }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }

                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = endpoint
                        .send_message_on(client_id, channel_id,ServerMessage::TeamDefined { team: team_to_insert }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                },
                ClientMessage::SettlementPlacementRequest { settlement, position } => {
                    let angle = 45.0_f32.to_radians();

                    let color;
                    if settlement.team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let material;

                    if let Some(mat) = materials.1.team_materials.get(&(resources.3.town_hall.0.id(), settlement.team)) {
                        material = mat.clone();
                    } else {
                        if let Some(original) = materials.0.get(resources.3.town_hall.1.id()) {
                            material = materials.2.add(ExtendedMaterial {
                                base: original.clone(),
                                extension: TeamMaterialExtension {
                                    team_color: color,
                                },
                            });
                        } else {
                            material = materials.2.add(ExtendedMaterial {
                                base: StandardMaterial{
                                    ..default()
                                },
                                extension: TeamMaterialExtension {
                                    team_color: color,
                                },
                            });
                        }

                        materials.1.team_materials.insert((resources.3.town_hall.0.id(), settlement.team), material.clone());
                    }

                    let new_settlement = commands.spawn(MaterialMeshBundle{
                        mesh: resources.3.town_hall.0.clone(),
                        material: material.clone(),
                        transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                        ..default()
                    })
                    .insert(SettlementComponent(settlement.clone()))
                    .id();

                    commands.spawn(CircleHolder(vec![
                        CircleData{
                            circle_center: position.xz(),
                            inner_radius: settlement.settlement_size,
                            outer_radius: settlement.settlement_size + 1.,
                            highlight_color: Vec4::new(1., 1., 1., 1.),
                        },
                        CircleData{
                            circle_center: position.xz(),
                            inner_radius: settlement.buffer_zone_size,
                            outer_radius: settlement.buffer_zone_size + 1.,
                            highlight_color: Vec4::new(1., 0., 0., 1.),
                        },
                        CircleData{
                            circle_center: position.xz(),
                            inner_radius: settlement.max_road_connection_distance,
                            outer_radius: settlement.max_road_connection_distance + 1.,
                            highlight_color: Vec4::new(0., 1., 0., 1.),
                        },
                    ]))
                    .insert(TemporaryObject);

                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::SettlementPlaced {
                            settlement: settlement.clone(),
                            position: position,
                            server_entity: new_settlement,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                },
                ClientMessage::AllSettlementsPlaced => {
                    if let Some(client) = clients.0.0.get_mut(&client_id) {
                        client.1 = true;
                    }
                },
                ClientMessage::BuildingPlacementRequest { team, name, position, angle, needed_buildpower } => {
                    if let Some(building) = resources.0.0.iter().find(|b| b.0 == name) {
                        let mut new_building_entity = Entity::PLACEHOLDER;
                        let transform = Transform::from_translation(Vec3::new(
                            position.x,
                            position.y + building.3,
                            position.z,
                        )).with_rotation(Quat::from_rotation_y(angle));

                        let color;
                        if team == 1 {
                            color = Vec4::new(0., 0., 1., 1.);
                        } else {
                            color = Vec4::new(1., 0., 0., 1.);
                        }

                        match &building.1 {
                            BuildingsBundles::InfantryBarracks(bundle) => {
                                let material;

                                if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                                }
                                
                                new_building_entity = commands.spawn(MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: transform,
                                    ..default()
                                }).insert(BuildingBlueprint{
                                    team: team,
                                    building_bundle: building.1.clone(),
                                    build_power_remaining: building.4,
                                    name: building.0.clone(),
                                    build_distance: building.5,
                                    resource_cost: building.6,
                                }).id();

                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::BlueprintPlaced{
                                        team: team,
                                        name: name.clone(),
                                        position: position,
                                        angle: angle,
                                        server_entity: new_building_entity,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            },
                            BuildingsBundles::VehicleFactory(bundle) => {
                                let material;

                                if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                                }

                                new_building_entity = commands.spawn(MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: transform,
                                    ..default()
                                }).insert(BuildingBlueprint{
                                    team: team,
                                    building_bundle: building.1.clone(),
                                    build_power_remaining: building.4,
                                    name: building.0.clone(),
                                    build_distance: building.5,
                                    resource_cost: building.6,
                                }).id();

                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::BlueprintPlaced{
                                        team: team,
                                        name: name.clone(),
                                        position: position,
                                        angle: angle,
                                        server_entity: new_building_entity,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            },
                            BuildingsBundles::LogisticHub(bundle) => {
                                let material;

                                if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                                }

                                new_building_entity = commands.spawn(MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: transform,
                                    ..default()
                                }).insert(BuildingBlueprint{
                                    team: team,
                                    building_bundle: building.1.clone(),
                                    build_power_remaining: building.4,
                                    name: building.0.clone(),
                                    build_distance: building.5,
                                    resource_cost: building.6,
                                }).id();

                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::BlueprintPlaced{
                                        team: team,
                                        name: name.clone(),
                                        position: position,
                                        angle: angle,
                                        server_entity: new_building_entity,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            },
                            BuildingsBundles::ResourceMiner(bundle) => {
                                for mut zone in queries.0.iter_mut() {
                                    zone.0.current_miners.entry(team).or_insert_with(|| None);

                                    let mut is_some = false;

                                    if let Some(mut miner) = zone.0.current_miners.get_mut(&team) {
                                        if let Some(entity) = miner {
                                            if commands.get_entity(entity.0).is_none() {
                                                miner = &mut None;
                                            } else {
                                                is_some = true;
                                            }
                                        }
                                    }
                                    
                                    if !is_some && zone.1.translation.xz().distance(position.xz()) <= zone.0.zone_radius {
                                        let material;

                                        if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                            material = mat.clone();
                                        } else {
                                            if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                                material = materials.2.add(ExtendedMaterial {
                                                    base: original.clone(),
                                                    extension: TeamMaterialExtension {
                                                        team_color: color,
                                                    },
                                                });
                                            } else {
                                                material = materials.2.add(ExtendedMaterial {
                                                    base: StandardMaterial{
                                                        ..default()
                                                    },
                                                    extension: TeamMaterialExtension {
                                                        team_color: color,
                                                    },
                                                });
                                            }

                                            materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                                        }
                                        
                                        new_building_entity = commands.spawn(MaterialMeshBundle{
                                            mesh: bundle.model.mesh.clone(),
                                            material: material.clone(),
                                            transform: transform,
                                            ..default()
                                        }).insert(BuildingBlueprint{
                                            team: team,
                                            building_bundle: building.1.clone(),
                                            build_power_remaining: building.4,
                                            name: building.0.clone(),
                                            build_distance: building.5,
                                            resource_cost: building.6,
                                        }).id();
        
                                        if let Some(miner) = zone.0.current_miners.get_mut(&team) {
                                            *miner = Some((new_building_entity, 0));
                                        }

                                        let mut channel_id = 60;
                                        while channel_id <= 89 {
                                            if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::BlueprintPlaced{
                                                team: team,
                                                name: name.clone(),
                                                position: position,
                                                angle: angle,
                                                server_entity: new_building_entity,
                                            }){
                                                channel_id += 1;
                                            } else {
                                                break;
                                            }
                                        }
        
                                        break;
                                    }
                                }
                            },
                            BuildingsBundles::Pillbox(bundle) => {
                                let material;

                                if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = materials.2.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                                }

                                new_building_entity = commands.spawn(MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: transform,
                                    ..default()
                                }).insert(BuildingBlueprint{
                                    team: team,
                                    building_bundle: building.1.clone(),
                                    build_power_remaining: building.4,
                                    name: building.0.clone(),
                                    build_distance: building.5,
                                    resource_cost: building.6,
                                }).id();

                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::BlueprintPlaced{
                                        team: team,
                                        name: name.clone(),
                                        position: position,
                                        angle: angle,
                                        server_entity: new_building_entity,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            BuildingsBundles::None => {},
                        }

                        if let GameStages::GameStarted = game_stage.0 {
                            unactivated_blueprints.0.entry(team).or_insert_with(HashMap::new)
                            .insert(new_building_entity, (position, Entity::PLACEHOLDER, building.5));
                        }
                    }
                },
                ClientMessage::ArmySetupStageCompleted { army } => {
                    if let Some(client) = clients.0.0.get_mut(&client_id) {
                        client.2 = true;

                        let mut regular_platoons: HashMap<(i32, i32, i32, i32, i32), (RegularSquad, String, Entity)> = HashMap::new();
                        let mut shock_platoons: HashMap<(i32, i32, i32, i32, i32), (ShockSquad, String, Entity)> = HashMap::new();
                        let mut armored_platoons: HashMap<(i32, i32, i32, i32, i32), (ArmoredSquad, String, Entity)> = HashMap::new();
                        let mut artillery_units: (HashMap<i32, ((Option<Entity>, String), Entity)>, Entity) = (HashMap::new(), Entity::PLACEHOLDER);
                        let mut engineers: HashMap<i32, ((Option<Entity>, String), Entity)> = HashMap::new();

                        for s_regular_platoon in army.regular_platoons.iter() {
                            let mut soldiers: LimitedHashSet<Entity, REGULAR_SQUAD_SIZE> = LimitedHashSet::new();
                            let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_REGULAR_SQUAD> = LimitedHashSet::new();

                            for soldier in s_regular_platoon.1.0.0.0.iter() {
                                let _ = soldiers.insert(*soldier);
                            }

                            for specialist in s_regular_platoon.1.0.0.1.iter() {
                                let _ = specialists.insert(*specialist);
                            }

                            regular_platoons.insert(s_regular_platoon.0, (RegularSquad((soldiers, specialists)), s_regular_platoon.1.1.clone(), s_regular_platoon.1.2));
                        }

                        for s_shock_platoon in army.shock_platoons.iter() {
                            let mut soldiers: LimitedHashSet<Entity, SHOCK_SQUAD_SIZE> = LimitedHashSet::new();
                            let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_SHOCK_SQUAD> = LimitedHashSet::new();

                            for soldier in s_shock_platoon.1.0.0.0.iter() {
                                let _ = soldiers.insert(*soldier);
                            }

                            for specialist in s_shock_platoon.1.0.0.1.iter() {
                                let _ = specialists.insert(*specialist);
                            }

                            shock_platoons.insert(s_shock_platoon.0, (ShockSquad((soldiers, specialists)), s_shock_platoon.1.1.clone(), s_shock_platoon.1.2));
                        }

                        for s_armored_platoon in army.armored_platoons.iter() {
                            let mut vehicles: LimitedHashSet<Entity, ARMORED_SQUAD_SIZE> = LimitedHashSet::new();

                            for vehicle in s_armored_platoon.1.0.0.iter() {
                                let _ = vehicles.insert(*vehicle);
                            }

                            armored_platoons.insert(s_armored_platoon.0, (ArmoredSquad(vehicles), s_armored_platoon.1.1.clone(), s_armored_platoon.1.2));
                        }

                        for s_artillery in army.artillery_units.0.iter() {
                            artillery_units.0.insert(s_artillery.0, s_artillery.1.clone());
                        }

                        for s_engineer in army.engineers.iter() {
                            engineers.insert(s_engineer.0, s_engineer.1.clone());
                        }

                        armies.0.insert(2, ArmyObject{
                            regular_squads: regular_platoons,
                            shock_squads: shock_platoons,
                            armored_squads: armored_platoons,
                            artillery_units,
                            engineers,
                        });

                        let mut add_amount;
                        for platoon in armies.0.get(&2).unwrap().regular_squads.iter() {
                            add_amount = platoon.1.0.0.0.capacity() - platoon.1.0.0.0.len();
            
                            for i in 0..add_amount as i32 {
                                production_queue.0.get_mut(&2).unwrap().regular_infantry_queue.insert(
                                    (
                                        platoon.0.0,
                                        platoon.0.1,
                                        platoon.0.2,
                                        platoon.0.3,
                                        platoon.0.4,
                                        0,
                                        i,
                                    ), (
                                        "regular_soldier".to_string(),
                                        Entity::PLACEHOLDER,
                                    ),
                                );
                            }
            
                            add_amount = platoon.1.0.0.1.capacity() - platoon.1.0.0.1.len();
            
                            for i in 0..add_amount as i32 {
                                production_queue.0.get_mut(&2).unwrap().regular_infantry_queue.insert(
                                    (
                                        platoon.0.0,
                                        platoon.0.1,
                                        platoon.0.2,
                                        platoon.0.3,
                                        platoon.0.4,
                                        1,
                                        i,
                                    ), (
                                        platoon.1.1.clone(),
                                        Entity::PLACEHOLDER,
                                    ),
                                );
                            }
                        }
            
                        for platoon in armies.0.get(&2).unwrap().shock_squads.iter() {
                            add_amount = platoon.1.0.0.0.capacity() - platoon.1.0.0.0.len();
            
                            for i in 0..add_amount as i32 {
                                production_queue.0.get_mut(&2).unwrap().shock_infantry_queue.insert(
                                    (
                                        platoon.0.0,
                                        platoon.0.1,
                                        platoon.0.2,
                                        platoon.0.3,
                                        platoon.0.4,
                                        0,
                                        i,
                                    ), (
                                        "shock_soldier".to_string(),
                                        Entity::PLACEHOLDER,
                                    ),
                                );
                            }
            
                            add_amount = platoon.1.0.0.1.capacity() - platoon.1.0.0.1.len();
            
                            for i in 0..add_amount as i32 {
                                production_queue.0.get_mut(&2).unwrap().shock_infantry_queue.insert(
                                    (
                                        platoon.0.0,
                                        platoon.0.1,
                                        platoon.0.2,
                                        platoon.0.3,
                                        platoon.0.4,
                                        1,
                                        i,
                                    ), (
                                        platoon.1.1.clone(),
                                        Entity::PLACEHOLDER,
                                    ),
                                );
                            }
                        }
            
                        for platoon in armies.0.get(&2).unwrap().armored_squads.iter() {
                            add_amount = platoon.1.0.0.capacity() - platoon.1.0.0.len();
            
                            for i in 0..add_amount as i32 {
                                production_queue.0.get_mut(&2).unwrap().vehicles_queue.insert(
                                    (
                                        platoon.0.0,
                                        platoon.0.1,
                                        platoon.0.2,
                                        platoon.0.3,
                                        platoon.0.4,
                                        0,
                                        i,
                                    ), (
                                        platoon.1.1.clone(),
                                        Entity::PLACEHOLDER,
                                    ),
                                );
                            }
                        }
            
                        for artillery_unit in armies.0.get(&2).unwrap().artillery_units.0.iter() {
                            if artillery_unit.1.0.0 == None {
                                production_queue.0.get_mut(&2).unwrap().artillery_queue.insert((0,0,0,0,0,0, *artillery_unit.0), (artillery_unit.1.0.1.clone(), Entity::PLACEHOLDER));
                            }
                        }
            
                        for engineer in armies.0.get(&2).unwrap().engineers.iter() {
                            if engineer.1.0.0 == None {
                                production_queue.0.get_mut(&2).unwrap().engineers_queue.insert((0,0,0,0,0,0, *engineer.0), (engineer.1.0.1.clone(), Entity::PLACEHOLDER));
                            }
                        }
                    }
                },
                ClientMessage::ClientArmyChanged { army } => {
                    let mut regular_platoons: HashMap<(i32, i32, i32, i32, i32), (RegularSquad, String, Entity)> = HashMap::new();
                    let mut shock_platoons: HashMap<(i32, i32, i32, i32, i32), (ShockSquad, String, Entity)> = HashMap::new();
                    let mut armored_platoons: HashMap<(i32, i32, i32, i32, i32), (ArmoredSquad, String, Entity)> = HashMap::new();
                    let mut artillery_units: (HashMap<i32, ((Option<Entity>, String), Entity)>, Entity) = (HashMap::new(), Entity::PLACEHOLDER);
                    let mut engineers: HashMap<i32, ((Option<Entity>, String), Entity)> = HashMap::new();

                    for s_regular_platoon in army.regular_platoons.iter() {
                        let mut soldiers: LimitedHashSet<Entity, REGULAR_SQUAD_SIZE> = LimitedHashSet::new();
                        let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_REGULAR_SQUAD> = LimitedHashSet::new();

                        for soldier in s_regular_platoon.1.0.0.0.iter() {
                            let _ = soldiers.insert(*soldier);
                        }

                        for specialist in s_regular_platoon.1.0.0.1.iter() {
                            let _ = specialists.insert(*specialist);
                        }

                        regular_platoons.insert(s_regular_platoon.0, (RegularSquad((soldiers, specialists)), s_regular_platoon.1.1.clone(), s_regular_platoon.1.2));
                    }

                    for s_shock_platoon in army.shock_platoons.iter() {
                        let mut soldiers: LimitedHashSet<Entity, SHOCK_SQUAD_SIZE> = LimitedHashSet::new();
                        let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_SHOCK_SQUAD> = LimitedHashSet::new();

                        for soldier in s_shock_platoon.1.0.0.0.iter() {
                            let _ = soldiers.insert(*soldier);
                        }

                        for specialist in s_shock_platoon.1.0.0.1.iter() {
                            let _ = specialists.insert(*specialist);
                        }

                        shock_platoons.insert(s_shock_platoon.0, (ShockSquad((soldiers, specialists)), s_shock_platoon.1.1.clone(), s_shock_platoon.1.2));
                    }

                    for s_armored_platoon in army.armored_platoons.iter() {
                        let mut vehicles: LimitedHashSet<Entity, ARMORED_SQUAD_SIZE> = LimitedHashSet::new();

                        for vehicle in s_armored_platoon.1.0.0.iter() {
                            let _ = vehicles.insert(*vehicle);
                        }

                        armored_platoons.insert(s_armored_platoon.0, (ArmoredSquad(vehicles), s_armored_platoon.1.1.clone(), s_armored_platoon.1.2));
                    }

                    for s_artillery in army.artillery_units.0.iter() {
                        artillery_units.0.insert(s_artillery.0, s_artillery.1.clone());
                    }

                    for s_engineer in army.engineers.iter() {
                        engineers.insert(s_engineer.0, s_engineer.1.clone());
                    }

                    armies.0.insert(2, ArmyObject{
                        regular_squads: regular_platoons,
                        shock_squads: shock_platoons,
                        armored_squads: armored_platoons,
                        artillery_units,
                        engineers,
                    });
                },
                ClientMessage::ProductionStateChanged { team, is_allowed } => {
                    production_state.is_allowed.entry(team).or_insert_with(|| is_allowed);

                    event_writer.0.send(ProductionStateChanged { team: team, is_allowed: is_allowed });
                },
                ClientMessage::UnitPathInsertRequest { entity, path } => {
                    if let Ok(mut unit) = moving_units_q.get_mut(entity) {
                        unit.path = path.clone();
                        commands.entity(entity).try_insert(NeedToMove);

                        let mut channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::UnitPathInserted {
                                server_entity: entity,
                                path: path.clone(),
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                },
                ClientMessage::ArtilleryDesignationRequest { artillery_entity, target_position: position } => {
                    commands.entity(artillery_entity).insert(ArtilleryNeedsToFire(position));
                },
                ClientMessage::CancelArtilleryFire { artillery_entity } => {
                    if let Ok(mut unit) = queries.1.get_mut(artillery_entity) {
                        commands.entity(artillery_entity).remove::<ArtilleryNeedsToFire>();
                        unit.elapsed_reload_time = 0;
                    }
                },
                ClientMessage::CoverAssignationRequest { units, cover_entity, cover_position } => {
                    for unit in units.iter() {
                        commands.entity(*unit).insert(MovingToCover{
                            cover_entity: cover_entity,
                            cover_position: cover_position,
                        });
                    }
                },
                ClientMessage::UncoveringRequest { unit_entities } => {
                    for unit_entity in unit_entities.iter() {
                        if commands.get_entity(*unit_entity).is_some() {
                            commands.entity(*unit_entity).remove::<MovingToCover>();
                        }

                        if let Ok(mut unit) = queries.3.get_mut(*unit_entity) {
                            if let Some(cover) = unit.1 {
                                event_writer.1.send(UnitNeedsToBeUncovered{
                                    cover_entity: cover.cover_entity,
                                    unit_entity: unit.0,
                                });

                                unit.2.translation.y = cover.original_y;
                                commands.entity(unit.0).remove::<Covered>();

                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::UnitUncovered {
                                        server_entity: unit.0,
                                        original_y: cover.original_y,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                },
                ClientMessage::DeleteUnspecifiedEntityRequest { entity } => {
                    commands.entity(entity).despawn_recursive();

                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                            server_entity: entity,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                },
                ClientMessage::DeconstructionRequest { entity, team, deconstruction_distance } => {
                    commands.entity(entity).insert(ToDeconstruct{
                        team: team,
                        deconstructor_entity: Entity::PLACEHOLDER,
                        progress_bar_entity: Entity::PLACEHOLDER,
                        deconstruction_distance: deconstruction_distance,
                    });

                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::DeconstructionAssigned {
                            server_entity: entity,
                            team: team,
                            deconstruction_distance: deconstruction_distance,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                },
                ClientMessage::DeconstructionCancelationRequest { entity, position } => {
                    commands.entity(entity).remove::<ToDeconstruct>();

                    if let Ok(mut deconstruction_site) = queries.5.get_mut(entity) {
                        deconstruction_site.buildpower_to_deconstruct_remaining = 0;
                    }

                    if let Ok(construction_site) = queries.4.get(entity) {
                        commands.entity(construction_site.current_builder).insert(BusyEngineer(
                            EngineerActions::Construction((position, entity, construction_site.build_distance))
                        ));
                    }

                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::DeconstructionCanceled {
                            server_entity: entity,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                },
                ClientMessage::BuildingStateSwitchRequest { entity, state } => {
                    if let Ok(mut building) = queries.6.get_mut(entity) {
                        building.0 = state;
                    }
                },
                ClientMessage::ApartmentsRebuildingRequest { entity } => {
                    if let Ok(mut settlement) = queries.7.get_mut(entity) {
                        if settlement.0.elapsed_capture_time > 0 {
                            return;
                        }

                        let team = settlement.0.team;
                        for apartment in settlement.0.active_apartments.iter_mut() {
                            if commands.get_entity(apartment.0).is_none() {
                                let mut total_materials = 0;

                                for material_producer in queries.8.iter() {
                                    if team != material_producer.1.team {continue;}

                                    total_materials += material_producer.0.available_materials;
                                }

                                if total_materials >= 5000 {
                                    let mut remaining_resource_cost = 5000;

                                    for mut material_producer in queries.8.iter_mut() {
                                        if team != material_producer.1.team {continue;}

                                        let current_remains = remaining_resource_cost - material_producer.0.available_materials;

                                        if current_remains <= 0 {
                                            material_producer.0.available_materials -= remaining_resource_cost;

                                            break;
                                        } else {
                                            remaining_resource_cost -= material_producer.0.available_materials;
                                            material_producer.0.available_materials = 0;
                                        }
                                    }
                                    
                                    let new_construction_tile = ((apartment.1.x / TILE_SIZE) as i32, (apartment.1.z / TILE_SIZE) as i32);

                                    let new_construction_site = commands.spawn(MaterialMeshBundle{
                                        mesh: resources.3.apartment.0.clone(),
                                        material: materials.1.blue_transparent.clone(),
                                        transform: Transform::from_translation(apartment.1).with_rotation(Quat::from_rotation_y(apartment.2)),
                                        ..default()
                                    })
                                    .insert(BuildingConstructionSite{
                                        team: team,
                                        building_bundle: BuildingsBundles::None,
                                        build_power_total: 200,
                                        build_power_remaining: 200,
                                        name: "".to_string(),
                                        build_distance: 30.,
                                        current_builder: Entity::PLACEHOLDER,
                                        resource_cost: 0,
                                    }).insert(CombatComponent{
                                        team: team,
                                        current_health: 10,
                                        max_health: 10,
                                        unit_type: UnitTypes::Building,
                                        attack_type: AttackTypes::None,
                                        attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                                        attack_frequency: 0,
                                        attack_elapsed_time: 0,
                                        detection_range: 0.,
                                        attack_range: 0.,
                                        enemies: vec![],
                                        is_static: true,
                                        unit_data: (
                                            new_construction_tile,
                                            (
                                                CompanyTypes::None,
                                                (0, 0, 0, 0, 0, 0, 0),
                                                "".to_string(),
                                            )
                                        ),
                                    })
                                    .insert(DontTouch)
                                    .id();

                                    apartment.0 = new_construction_site;

                                    resources.2.tiles.entry(team).or_insert_with(HashMap::new).entry(new_construction_tile)
                                    .or_insert_with(HashMap::new).insert(new_construction_site, (apartment.1, UnitTypes::None));

                                    let bar_size = resources.1.button_size * 0.75;

                                    commands.spawn(NodeBundle{
                                        style: Style {
                                            position_type: PositionType::Relative,
                                            width: Val::Px(bar_size),
                                            height: Val::Px(bar_size / 4.),
                                            flex_direction: FlexDirection::Column,
                                            justify_content: JustifyContent::Start,
                                            align_items: AlignItems::Start,
                                            top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                            ..default()
                                        },
                                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                        ..default()
                                    })
                                    .insert(Visibility::Hidden)
                                    .with_children(|parent| {
                                        parent.spawn(NodeBundle {
                                            style: Style {
                                                position_type: PositionType::Relative,
                                                width: Val::Px(0.),
                                                height: Val::Px(bar_size / 4.),
                                                flex_direction: FlexDirection::Column,
                                                justify_content: JustifyContent::Start,
                                                align_items: AlignItems::Start,
                                                ..default()
                                            },
                                            background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                            ..default()
                                        })
                                        .insert(ConstructionProgressBar {
                                            constrcution_entity: new_construction_site,
                                            max_width: bar_size,
                                        });
                                    });

                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::ApartmentConstructionSitePlaced {
                                            server_entity: new_construction_site,
                                            position: apartment.1,
                                            angle: apartment.2,
                                            team: team,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                ClientMessage::TransportAssignationRequest { units, transport_entity, transport_position } => {
                    for unit_entity in units.iter() {
                        commands.entity(*unit_entity).insert(MovingToTransport{
                            transport_entity: transport_entity,
                            transport_position: transport_position,
                        });
                    }
                },
                ClientMessage::TransportAssignationCancelRequest { units } => {
                    for unit_entity in units.iter() {
                        commands.entity(*unit_entity).remove::<MovingToTransport>();
                        commands.entity(*unit_entity).remove::<NeedToMove>();
                    }
                },
                ClientMessage::DisembarkRequest { transports } => {
                    for transport_entity in transports.iter() {
                        if let Ok(mut transport) = queries.9.get_mut(*transport_entity) {
                            let mut disembarked_units: Vec<Entity> = Vec::new();

                            for unit_entity in transport.0.units_inside.iter() {
                                if let Ok(mut unit) = queries.10.get_mut(*unit_entity) {
                                    commands.entity(*unit_entity).remove::<DisabledUnit>();
                                    commands.entity(*unit_entity).remove::<InTransport>();

                                    unit.translation = transport.1.translation + Vec3::new(0., 0., 0.);

                                    disembarked_units.push(*unit_entity);
                                }
                            }

                            transport.0.units_inside.clear();

                            let mut channel_id = 30;
                            while channel_id <= 59 {
                                if let Err(_) = endpoint.send_group_message_on(clients.0.0.keys(), channel_id, ServerMessage::UnitsDisembarked {
                                    server_entities: disembarked_units.clone(),
                                    transport_server_entity: *transport_entity,
                                    transport_position: transport.1.translation,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }

                            let mut origin_position = -transport.1.forward() * 20. + transport.1.translation;

                            let mut counter = 0;
                            let mut operation_counter = 0;
                            let mut operation_number = 1;   //\/
                            let mut z_minus = 2;            //1
                            let mut x_minus = 2;            //2
                            let mut z_plus = 3;             //3
                            let mut x_plus = 3;             //4
                            let offset = 5.;

                            for unit_entity in disembarked_units.iter(){
                                if let Ok(mut unit) = moving_units_q.get_mut(*unit_entity){
                                    match counter {
                                        0 => {}
                                        1 => origin_position.z += offset,
                                        2 => origin_position.x += offset,
                                        _ =>
                                        match operation_number {
                                            1 => {
                                                origin_position.z -= offset;
                                                operation_counter += 1;
                                                if operation_counter == z_minus {
                                                    operation_counter = 0;
                                                    z_minus += 2;
                                                    operation_number = 2;
                                                }
                                            },
                                            2 => {
                                                origin_position.x -= offset;
                                                operation_counter += 1;
                                                if operation_counter == x_minus {
                                                    operation_counter = 0;
                                                    x_minus += 2;
                                                    operation_number = 3;
                                                }
                                            }
                                            3 => {
                                                origin_position.z += offset;
                                                operation_counter += 1;
                                                if operation_counter == z_plus {
                                                    operation_counter = 0;
                                                    z_plus += 2;
                                                    operation_number = 4;
                                                }
                                            }
                                            4 => {
                                                origin_position.x += offset;
                                                operation_counter += 1;
                                                if operation_counter == x_plus {
                                                    operation_counter = 0;
                                                    x_plus += 2;
                                                    operation_number = 1;
                                                }
                                            }
                                            _ => {},
                                        }
                                    }

                                    unit.path = Vec::new();

                                    if let Ok(transform) = queries.10.get(*unit_entity) {
                                        unstarted_tasks.0.push((
                                            TaskPoolTypes::Manual,
                                            (
                                                transform.translation,
                                                origin_position,
                                                Some(100.),
                                                *unit_entity,
                                            ),
                                        ));
                                    }
                    
                                    counter += 1;
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

pub fn mp_settlements_placement_completion(
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    player_data: Res<PlayerData>,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<AllSettlementsPlaced>,
        EventWriter<DeleteTemporaryObjects>,
    ),
    mut game_stage: ResMut<GameStage>,
){
    if matches!(game_stage.0, GameStages::SettlementsSetup) && player_data.is_all_settlements_placed {
        let mut is_all_settlements_placed = true;

        for client in clients.0.iter() {
            if !client.1.1 {
                is_all_settlements_placed = false;
            }
        }

        if is_all_settlements_placed {
            event_writer.0.send(AllSettlementsPlaced);
            event_writer.1.send(DeleteTemporaryObjects);
            game_stage.0 = GameStages::BuildingsSetup;

            let mut channel_id = 60;
            while channel_id <= 89 {
                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::AllSettlementsPlaced){
                    channel_id += 1;
                } else {
                    break;
                }
            }
        }
    }
}

pub fn mp_game_starter(
    player_data: Res<PlayerData>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    game_stage: Res<GameStage>,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<GameStartedEvent>,
    )
){
    if player_data.is_ready_to_start && matches!(game_stage.0, GameStages::ArmySetup) {
        let mut is_all_ready = true;
        for client in clients.0.iter() {
            if !client.1.2 {
                is_all_ready = false;
            }
        }

        if is_all_ready {
            event_writer.0.send(GameStartedEvent);

            let mut channel_id = 60;
            while channel_id <= 89 {
                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::GameStarted){
                    channel_id += 1;
                } else {
                    break;
                }
            }
        }
    }
}

pub fn start_connection_to_server(
    mut client: ResMut<QuinnetClient>,
    ip_buffer: Res<InsertedConnectionData>,
) {
    if ip_buffer.ip == "".to_string() {
        let _ = client.close_all_connections();

        client
        .open_connection(
            ClientEndpointConfiguration::from_ips(SERVER_HOST, SERVER_PORT, LOCAL_BIND_IP, 0),
            CertificateVerificationMode::SkipVerification,
            ChannelsConfiguration::from_types(vec![
                    ChannelType::Unreliable,            //0
                    ChannelType::Unreliable,            //1
                    ChannelType::Unreliable,            //2
                    ChannelType::Unreliable,            //3
                    ChannelType::Unreliable,            //4
                    ChannelType::Unreliable,            //5
                    ChannelType::Unreliable,            //6
                    ChannelType::Unreliable,            //7
                    ChannelType::Unreliable,            //8
                    ChannelType::Unreliable,            //9
                    ChannelType::Unreliable,            //10
                    ChannelType::Unreliable,            //11
                    ChannelType::Unreliable,            //12
                    ChannelType::Unreliable,            //13
                    ChannelType::Unreliable,            //14
                    ChannelType::Unreliable,            //15
                    ChannelType::Unreliable,            //16
                    ChannelType::Unreliable,            //17
                    ChannelType::Unreliable,            //18
                    ChannelType::Unreliable,            //19
                    ChannelType::Unreliable,            //20
                    ChannelType::Unreliable,            //21
                    ChannelType::Unreliable,            //22
                    ChannelType::Unreliable,            //23
                    ChannelType::Unreliable,            //24
                    ChannelType::Unreliable,            //25
                    ChannelType::Unreliable,            //26
                    ChannelType::Unreliable,            //27
                    ChannelType::Unreliable,            //28
                    ChannelType::Unreliable,            //29

                    ChannelType::UnorderedReliable,     //30
                    ChannelType::UnorderedReliable,     //31
                    ChannelType::UnorderedReliable,     //32
                    ChannelType::UnorderedReliable,     //33
                    ChannelType::UnorderedReliable,     //34
                    ChannelType::UnorderedReliable,     //35
                    ChannelType::UnorderedReliable,     //36
                    ChannelType::UnorderedReliable,     //37
                    ChannelType::UnorderedReliable,     //38
                    ChannelType::UnorderedReliable,     //39
                    ChannelType::UnorderedReliable,     //40
                    ChannelType::UnorderedReliable,     //41
                    ChannelType::UnorderedReliable,     //42
                    ChannelType::UnorderedReliable,     //43
                    ChannelType::UnorderedReliable,     //44
                    ChannelType::UnorderedReliable,     //45
                    ChannelType::UnorderedReliable,     //46
                    ChannelType::UnorderedReliable,     //47
                    ChannelType::UnorderedReliable,     //48
                    ChannelType::UnorderedReliable,     //49
                    ChannelType::UnorderedReliable,     //50
                    ChannelType::UnorderedReliable,     //51
                    ChannelType::UnorderedReliable,     //52
                    ChannelType::UnorderedReliable,     //53
                    ChannelType::UnorderedReliable,     //54
                    ChannelType::UnorderedReliable,     //55
                    ChannelType::UnorderedReliable,     //56
                    ChannelType::UnorderedReliable,     //57
                    ChannelType::UnorderedReliable,     //58
                    ChannelType::UnorderedReliable,     //59

                    ChannelType::OrderedReliable,       //60
                    ChannelType::OrderedReliable,       //61
                    ChannelType::OrderedReliable,       //62
                    ChannelType::OrderedReliable,       //63
                    ChannelType::OrderedReliable,       //64
                    ChannelType::OrderedReliable,       //65
                    ChannelType::OrderedReliable,       //66
                    ChannelType::OrderedReliable,       //67
                    ChannelType::OrderedReliable,       //68
                    ChannelType::OrderedReliable,       //69
                    ChannelType::OrderedReliable,       //70
                    ChannelType::OrderedReliable,       //71
                    ChannelType::OrderedReliable,       //72
                    ChannelType::OrderedReliable,       //73
                    ChannelType::OrderedReliable,       //74
                    ChannelType::OrderedReliable,       //75
                    ChannelType::OrderedReliable,       //76
                    ChannelType::OrderedReliable,       //77
                    ChannelType::OrderedReliable,       //78
                    ChannelType::OrderedReliable,       //79
                    ChannelType::OrderedReliable,       //80
                    ChannelType::OrderedReliable,       //81
                    ChannelType::OrderedReliable,       //82
                    ChannelType::OrderedReliable,       //83
                    ChannelType::OrderedReliable,       //84
                    ChannelType::OrderedReliable,       //85
                    ChannelType::OrderedReliable,       //86
                    ChannelType::OrderedReliable,       //87
                    ChannelType::OrderedReliable,       //88
                    ChannelType::OrderedReliable,       //89
                ])
                .unwrap(),
        )
        .unwrap();
    } else {
        let ip: Vec<&str> = ip_buffer.ip.split(':').collect();
        println!("{}", ip[0]);
        let server_host = Ipv4Addr::from_str(&ip[0]).unwrap();
        let server_port = ip[1].parse::<u16>().unwrap();
        
        let _ = client.close_all_connections();

        client
        .open_connection(
            ClientEndpointConfiguration::from_ips(server_host, server_port, LOCAL_BIND_IP, 0),
            CertificateVerificationMode::SkipVerification,
            ChannelsConfiguration::from_types(vec![
                ChannelType::Unreliable,            //0
                ChannelType::Unreliable,            //1
                ChannelType::Unreliable,            //2
                ChannelType::Unreliable,            //3
                ChannelType::Unreliable,            //4
                ChannelType::Unreliable,            //5
                ChannelType::Unreliable,            //6
                ChannelType::Unreliable,            //7
                ChannelType::Unreliable,            //8
                ChannelType::Unreliable,            //9
                ChannelType::Unreliable,            //10
                ChannelType::Unreliable,            //11
                ChannelType::Unreliable,            //12
                ChannelType::Unreliable,            //13
                ChannelType::Unreliable,            //14
                ChannelType::Unreliable,            //15
                ChannelType::Unreliable,            //16
                ChannelType::Unreliable,            //17
                ChannelType::Unreliable,            //18
                ChannelType::Unreliable,            //19
                ChannelType::Unreliable,            //20
                ChannelType::Unreliable,            //21
                ChannelType::Unreliable,            //22
                ChannelType::Unreliable,            //23
                ChannelType::Unreliable,            //24
                ChannelType::Unreliable,            //25
                ChannelType::Unreliable,            //26
                ChannelType::Unreliable,            //27
                ChannelType::Unreliable,            //28
                ChannelType::Unreliable,            //29

                ChannelType::UnorderedReliable,     //30
                ChannelType::UnorderedReliable,     //31
                ChannelType::UnorderedReliable,     //32
                ChannelType::UnorderedReliable,     //33
                ChannelType::UnorderedReliable,     //34
                ChannelType::UnorderedReliable,     //35
                ChannelType::UnorderedReliable,     //36
                ChannelType::UnorderedReliable,     //37
                ChannelType::UnorderedReliable,     //38
                ChannelType::UnorderedReliable,     //39
                ChannelType::UnorderedReliable,     //40
                ChannelType::UnorderedReliable,     //41
                ChannelType::UnorderedReliable,     //42
                ChannelType::UnorderedReliable,     //43
                ChannelType::UnorderedReliable,     //44
                ChannelType::UnorderedReliable,     //45
                ChannelType::UnorderedReliable,     //46
                ChannelType::UnorderedReliable,     //47
                ChannelType::UnorderedReliable,     //48
                ChannelType::UnorderedReliable,     //49
                ChannelType::UnorderedReliable,     //50
                ChannelType::UnorderedReliable,     //51
                ChannelType::UnorderedReliable,     //52
                ChannelType::UnorderedReliable,     //53
                ChannelType::UnorderedReliable,     //54
                ChannelType::UnorderedReliable,     //55
                ChannelType::UnorderedReliable,     //56
                ChannelType::UnorderedReliable,     //57
                ChannelType::UnorderedReliable,     //58
                ChannelType::UnorderedReliable,     //59

                ChannelType::OrderedReliable,       //60
                ChannelType::OrderedReliable,       //61
                ChannelType::OrderedReliable,       //62
                ChannelType::OrderedReliable,       //63
                ChannelType::OrderedReliable,       //64
                ChannelType::OrderedReliable,       //65
                ChannelType::OrderedReliable,       //66
                ChannelType::OrderedReliable,       //67
                ChannelType::OrderedReliable,       //68
                ChannelType::OrderedReliable,       //69
                ChannelType::OrderedReliable,       //70
                ChannelType::OrderedReliable,       //71
                ChannelType::OrderedReliable,       //72
                ChannelType::OrderedReliable,       //73
                ChannelType::OrderedReliable,       //74
                ChannelType::OrderedReliable,       //75
                ChannelType::OrderedReliable,       //76
                ChannelType::OrderedReliable,       //77
                ChannelType::OrderedReliable,       //78
                ChannelType::OrderedReliable,       //79
                ChannelType::OrderedReliable,       //80
                ChannelType::OrderedReliable,       //81
                ChannelType::OrderedReliable,       //82
                ChannelType::OrderedReliable,       //83
                ChannelType::OrderedReliable,       //84
                ChannelType::OrderedReliable,       //85
                ChannelType::OrderedReliable,       //86
                ChannelType::OrderedReliable,       //87
                ChannelType::OrderedReliable,       //88
                ChannelType::OrderedReliable,       //89
            ])
            .unwrap(),
        )
        .unwrap();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ServerMessage{
    PlayerJoined{
        player_list: Vec<(i32, Vec<(ClientId, String)>)>,
    },
    PlayerQuit{
        player_list: Vec<(i32, Vec<(ClientId, String)>)>,
    },
    TeamDefined{
        team: i32,
    },
    SettlementPlaced{
        settlement: SettlementObject,
        position: Vec3,
        server_entity: Entity,
    },
    ApartmentGenerated{
        team: i32,
        server_entity: Entity,
        position: Vec3,
        angle: f32,
    },
    RoadGenerated{
        road_points: Vec<Vec3>,
        road_center: Vec3,
        server_entity: Entity,
    },
    ResourceZonePlaced{
        position: Vec3,
        server_entity: Entity,
    },
    BlueprintPlaced{
        team: i32,
        name: String,
        position: Vec3,
        angle: f32,
        server_entity: Entity,
    },
    ConstructionSiteBuilt{
        team: i32,
        name: String,
        position: Vec3,
        blueprint_server_entity: Entity,
        angle: f32,
        server_entity: Entity,
    },
    BuildingBuilt{
        team: i32,
        name: String,
        position: Vec3,
        construction_site_server_entity: Entity,
        angle: f32,
        server_entity: Entity,
    },
    UnitSpawned{
        unit_data: (
            i32,                //team
            (
                CompanyTypes, //battalion type
                (
                    i32,        //division index
                    i32,        //brigade index
                    i32,        //batallion index
                    i32,        //company index
                    i32,        //platoon index
                    i32,        //sub-platoon index
                    i32,        //unit index
                ),
                String,         //unit name
            ),
        ),
        position: Vec3,
        server_entity: Entity,
    },
    UnitPathInserted{
        server_entity: Entity,
        path: Vec<Vec3>,
    },
    UnitDamaged{
        server_entity: Entity,
        damage: i32,
    },
    UnitRemoved{
        server_entity: Entity,
        unit_data: (
            i32,                //team
            (i32, i32),         //tile key
            (
                CompanyTypes, //battalion type
                (
                    i32,        //division index
                    i32,        //brigade index
                    i32,        //batallion index
                    i32,        //company index
                    i32,        //platoon index
                    i32,        //sub-platoon index
                    i32,        //unit index
                ),
                String,         //unit name
            ),
        ),
        should_spawn_corpse: bool,
    },
    ArtilleryProjectileSpawned{
        position:Vec3,
        server_entity: Entity,
    },
    HomingProjectileSpawned{
        position:Vec3,
        server_entity: Entity,
    },
    LogisticUnitSpawned{
        position: Vec3,
        server_entity: Entity,
        team: i32,
    },
    UnspecifiedEntityMoved{
        server_entity: Entity,
        new_position: Vec3,
    },
    UnspecifiedEntityRemoved{
        server_entity: Entity,
    },
    SettlementCaptured{
        server_entity: Entity,
        team: i32,
        captured_apartments: Vec<Entity>,
    },
    ExplosionOccured{
        position: Vec3,
    },
    GameInitialized,
    AllSettlementsPlaced,
    GameStarted,
    UnitCovered{
        server_entity: Entity,
        initial_unit_position_y: f32,
    },
    UnitUncovered{
        server_entity: Entity,
        original_y: f32,
    },
    UnitsSupplied{
        server_entities: Vec<(Entity, i32)>,
    },
    MaterialsDelivered{
        server_entity: Entity,
        amount: i32,
    },
    HumanResourcesDelivered{
        server_entity: Entity,
        amount: i32,
    },
    ConstructionProgressChanged{
        server_entity: Entity,
        current_build_power: i32,
    },
    DeconstructionProgressChanged{
        server_entity: Entity,
        current_build_power: i32,
    },
    DeconstructionAssigned{
        server_entity: Entity,
        team: i32,
        deconstruction_distance: f32,
    },
    DeconstructionStarted{
        server_deconstruction_entity: Entity,
    },
    DeconstructionCanceled{
        server_entity: Entity,
    },
    ApartmentConstructionSitePlaced{
        server_entity: Entity,
        position: Vec3,
        angle: f32,
        team: i32,
    },
    SettlementCaptureStarted{
        settlement_server_entity: Entity,
    },
    SettlementCaptureEnded{
        settlement_server_entity: Entity,
    },
    ResourceDisplayesUpdated{
        materials_display: (String, String),
        human_resource_display: (String, String),
    },
    TransportAssignationCanceled{
        server_entities: Vec<Entity>,
    },
    UnitsEmbarked{
        server_entities: Vec<(Entity, (i32, i32))>,
        transport_server_entity: Entity,
        team: i32,
    },
    UnitsDisembarked{
        server_entities: Vec<Entity>,
        transport_server_entity: Entity,
        transport_position: Vec3,
    },
    HostArmyChanged{
        army: SerializableArmyObject,
    },
}

#[derive(Resource)]
pub struct UnspecifiedEntitiesToMove(pub Vec<(Entity, Vec3)>);

#[derive(Resource)]
pub struct UnitsToDamage(pub Vec<(Entity, i32)>);

#[derive(Resource)]
pub struct UnitsToInsertPath(pub Vec<(Entity, Vec<Vec3>)>);

pub fn server_messages_handler(
    mut client: ResMut<QuinnetClient>,
    mut connection_events: (
        EventReader<ConnectionEvent>,
        EventReader<ConnectionFailedEvent>,
    ),
    ip_buffer: Res<InsertedConnectionData>,
    mut other: (
        ResMut<PlayerList>,
        ResMut<PlayerData>,
        ResMut<NextState<GameState>>,
    ),
    mut entity_maps: ResMut<EntityMaps>,
    mut entities_to_move: (
        ResMut<UnspecifiedEntitiesToMove>,
        ResMut<UnitsToInsertPath>,
    ),
    mut materials: (
        ResMut<Assets<StandardMaterial>>,
        ResMut<InstancedMaterials>,
        ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    ),
    mut queries: (
        Query<(Entity, &Transform, &mut ResourceZone), With<ResourceZone>>,
        Query<&mut Transform, Without<ResourceZone>>,
        Query<&mut SuppliesConsumerComponent>,
        Query<&mut MaterialsStorageComponent>,
        Query<&mut HumanResourceStorageComponent>,
        Query<&mut BuildingConstructionSite>,
        Query<&mut DeconstructableBuilding>,
        Query<&mut ToDeconstruct>,
        Query<&mut SettlementComponent>,
        Query<&mut Text, (With<MaterialsOverallAmountDisplay>, Without<HumanResourcesOverallAmountDisplay>)>,
        Query<&mut Text, (With<HumanResourcesOverallAmountDisplay>, Without<MaterialsOverallAmountDisplay>)>,
        Query<&mut CombatComponent, With<ApartmentHouse>>,
        Query<&mut InfantryTransport>,
    ),
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    buildings_list: Res<BuildingsList>,
    mut armies: ResMut<Armies>,
    producable_units: Res<ProducableUnits>,
    mut resources: (
        Res<BuildingsAssets>,
        Res<AttackVisualisationAssets>,
        Res<UnitAssets>,
        Res<UiButtonNodes>,
        ResMut<RemainsCount>,
        ResMut<Settlements>,
    ),
    mut tile_map: ResMut<UnitsTileMap>,
    mut event_writer: (
        EventWriter<ClientGameInitializedEvent>,
        EventWriter<AllPlayersPlacedSettlementsEvent>,
        EventWriter<ClientGameStartedEvent>,
        EventWriter<ExplosionEvent>,
    ),
) {
    if !connection_events.0.is_empty() {
        let username: String = ip_buffer.username.clone();

        client
            .connection_mut()
            .send_message_on(2, ClientMessage::Connected { name: username })
            .unwrap();

        connection_events.0.clear();
    }
    for ev in connection_events.1.read() {
        println!(
            "Failed to connect: {:?}.",
            ev.err
        );
    }

    while let Some((_, message)) = client
        .connection_mut()
        .try_receive_message::<ServerMessage>()
    {
        let mut sounds_counter = 0;
        match message {
            ServerMessage::PlayerJoined { player_list } => {
                let mut actual_player_list: HashMap<i32, HashMap<ClientId, String>> = HashMap::new();

                for team in player_list.iter(){
                    let mut players_to_insert: HashMap<ClientId, String> = HashMap::new();

                    for player in team.1.iter() {
                        players_to_insert.insert(player.0, player.1.clone());
                    }

                    actual_player_list.insert(team.0, players_to_insert);
                }

                other.0.0 = actual_player_list;
            },
            ServerMessage::PlayerQuit { player_list } => {
                let mut actual_player_list: HashMap<i32, HashMap<ClientId, String>> = HashMap::new();

                for team in player_list.iter(){
                    let mut players_to_insert: HashMap<ClientId, String> = HashMap::new();

                    for player in team.1.iter() {
                        players_to_insert.insert(player.0, player.1.clone());
                    }

                    actual_player_list.insert(team.0, players_to_insert);
                }

                other.0.0 = actual_player_list;
            },
            ServerMessage::TeamDefined { team } => {
                other.1.team = team;
            },
            ServerMessage::SettlementPlaced { settlement, position, server_entity } => {
                let color;
                if settlement.team == 1 {
                    color = Vec4::new(0., 0., 1., 1.);
                } else {
                    color = Vec4::new(1., 0., 0., 1.);
                }
                
                let material;

                if let Some(mat) =
                materials.1.team_materials.get(&(resources.0.town_hall.0.id(), settlement.team)) {
                    material = mat.clone();
                } else {
                    if let Some(original) = materials.0.get(resources.0.town_hall.1.id()) {
                        material = materials.2.add(ExtendedMaterial {
                            base: original.clone(),
                            extension: TeamMaterialExtension {
                                team_color: color,
                            },
                        });
                    } else {
                        material = materials.2.add(ExtendedMaterial {
                            base: StandardMaterial{
                                ..default()
                            },
                            extension: TeamMaterialExtension {
                                team_color: color,
                            },
                        });
                    }

                    materials.1.team_materials.insert((resources.0.town_hall.0.id(), settlement.team), material.clone());
                }

                let angle = 45.0_f32.to_radians();
                
                let client_entity = commands.spawn(MaterialMeshBundle{
                    mesh: resources.0.town_hall.0.clone(),
                    material: material.clone(),
                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                    ..default()
                })
                .insert(SettlementComponent(settlement.clone()))
                .id();

                commands.spawn(CircleHolder(vec![
                    CircleData{
                        circle_center: position.xz(),
                        inner_radius: settlement.settlement_size,
                        outer_radius: settlement.settlement_size + 1.,
                        highlight_color: Vec4::new(1., 1., 1., 1.),
                    },
                    CircleData{
                        circle_center: position.xz(),
                        inner_radius: settlement.buffer_zone_size,
                        outer_radius: settlement.buffer_zone_size + 1.,
                        highlight_color: Vec4::new(1., 0., 0., 1.),
                    },
                    CircleData{
                        circle_center: position.xz(),
                        inner_radius: settlement.max_road_connection_distance,
                        outer_radius: settlement.max_road_connection_distance + 1.,
                        highlight_color: Vec4::new(0., 1., 0., 1.),
                    },
                ]))
                .insert(TemporaryObject);

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::ApartmentGenerated { team, position, server_entity , angle} => {
                let client_entity = commands.spawn(MaterialMeshBundle{
                    mesh: resources.0.apartment.0.clone(),
                    material: resources.0.apartment.1.clone(),
                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                    ..default()
                })
                .insert(Collider::cuboid(18., 10., 8.))
                .insert(CollisionGroups::new(Group::GROUP_2, Group::all()))
                .insert(NavMeshAffector)
                .insert(NavMeshAreaType(None))
                .insert(ApartmentHouse)
                .insert(CombatComponent{
                    team: team,
                    current_health: 1000,
                    max_health: 1000,
                    unit_type: UnitTypes::Building,
                    attack_type: AttackTypes::None,
                    attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                    attack_frequency: 0,
                    attack_elapsed_time: 0,
                    detection_range: 0.,
                    attack_range: 0.,
                    enemies: Vec::new(),
                    is_static: true,
                    unit_data: (
                        (0,0),
                        (
                            CompanyTypes::None,
                            (0, 0, 0, 0, 0, 0, 0),
                            "".to_string(),
                        ),
                    ),
                })
                .insert(CoverComponent{
                    cover_efficiency: 0.5,
                    points: vec![position, position, position, position, position, position, position, position, position],
                    units_inside: HashSet::new(),
                })
                .id();

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::RoadGenerated { road_points, road_center, server_entity } => {
                let raod_mesh = create_curved_mesh(
                    5.,
                    5.,
                    road_points,
                    -2.9,
                    &Transform::from_translation(road_center),
                );

                let client_entity = commands.spawn(MaterialMeshBundle{
                    mesh: meshes.add(raod_mesh.clone()),
                    material: materials.0.add(Color::srgb(0.5, 0.5, 0.5)).into(),
                    transform: Transform::from_translation(road_center),
                    ..default()
                })
                .insert(Collider::from_bevy_mesh(&raod_mesh, &ComputedColliderShape::TriMesh).unwrap())
                .insert(NavMeshAffector)
                .insert(NavMeshAreaType(Some(Area(1))))
                .insert(NotShadowCaster)
                .insert(CollisionGroups::new(Group::GROUP_2, Group::all()))
                .id();

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::ResourceZonePlaced { position, server_entity } => {
                let resource_zone_size = 30.;

                let client_entity = commands.spawn(Transform::from_translation(position))
                .insert(ResourceZone{
                    zone_radius: resource_zone_size,
                    current_miners: HashMap::new(),
                })
                .insert(CircleHolder(vec![
                    CircleData{
                        circle_center: position.xz(),
                        inner_radius: resource_zone_size,
                        outer_radius: resource_zone_size + 1.,
                        highlight_color: Vec4::new(0., 1., 0., 1.),
                    },
                ]))
                .id();

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::BlueprintPlaced { team, name, position, angle, server_entity } => {
                if let Some(building) = buildings_list.0.iter().find(|b| b.0 == name) {
                    let mut client_entity = Entity::PLACEHOLDER;
                    let transform = Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle));

                    let color;
                    if team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }
                    
                    match &building.1 {
                        BuildingsBundles::InfantryBarracks(bundle) => {
                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: materials.1.blue_transparent.clone(),
                                transform: transform,
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_remaining: building.4,
                                name: building.0.clone(),
                                build_distance: building.5,
                                resource_cost: building.6,
                            }).id();
                        },
                        BuildingsBundles::VehicleFactory(bundle) => {
                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: materials.1.blue_transparent.clone(),
                                transform: transform,
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_remaining: building.4,
                                name: building.0.clone(),
                                build_distance: building.5,
                                resource_cost: building.6,
                            }).id();
                        },
                        BuildingsBundles::LogisticHub(bundle) => {
                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: materials.1.blue_transparent.clone(),
                                transform: transform,
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_remaining: building.4,
                                name: building.0.clone(),
                                build_distance: building.5,
                                resource_cost: building.6,
                            }).id();
                        },
                        BuildingsBundles::ResourceMiner(bundle) => {
                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: materials.1.blue_transparent.clone(),
                                transform: transform,
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_remaining: building.4,
                                name: building.0.clone(),
                                build_distance: building.5,
                                resource_cost: building.6,
                            }).id();

                            let mut nearest_zone = (f32::INFINITY, Entity::PLACEHOLDER);

                            for zone in queries.0.iter() {
                                let distance = zone.1.translation.distance(position);

                                if distance < nearest_zone.0 {
                                    nearest_zone.0 = distance;
                                    nearest_zone.1 = zone.0;
                                }
                            }

                            if let Ok(mut zone) = queries.0.get_mut(nearest_zone.1) {
                                zone.2.current_miners.entry(team).or_insert_with(|| None);
                                if let Some(miners) = zone.2.current_miners.get_mut(&team) {
                                    *miners = Some((client_entity, 1));
                                }
                            }
                        },
                        BuildingsBundles::Pillbox(bundle) => {
                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: materials.1.blue_transparent.clone(),
                                transform: transform,
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_remaining: building.4,
                                name: building.0.clone(),
                                build_distance: building.5,
                                resource_cost: building.6,
                            }).id();
                        },
                        BuildingsBundles::None => {},
                    }

                    entity_maps.client_to_server.insert(client_entity, server_entity);
                    entity_maps.server_to_client.insert(server_entity, client_entity);
                }
            },
            ServerMessage::ConstructionSiteBuilt { team, blueprint_server_entity, server_entity, name, position, angle } => {
                if let Some(bp_entity) = entity_maps.server_to_client.clone().get(&blueprint_server_entity) {
                    commands.entity(*bp_entity).despawn();
                    entity_maps.client_to_server.remove(bp_entity);
                    entity_maps.server_to_client.remove(&blueprint_server_entity);
                }

                if let Some(building) = buildings_list.0.iter().find(|b| b.0 == name) {
                    let mut client_entity = Entity::PLACEHOLDER;
                    let new_construction_tile = ((position.x / TILE_SIZE) as i32, (position.z / TILE_SIZE) as i32);

                    let mut unit_type = UnitTypes::None;

                    let color;
                    if team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let bar_size = resources.3.button_size * 0.75;

                    match &building.1 {
                        BuildingsBundles::InfantryBarracks(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingConstructionSite{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_total: building.4,
                                build_power_remaining: building.4,
                                name: name.clone(),
                                build_distance: building.5,
                                current_builder: Entity::PLACEHOLDER,
                                resource_cost: building.6,
                            }).insert(CombatComponent{
                                team: team,
                                current_health: bundle.combat_component.current_health / 10,
                                max_health: bundle.combat_component.current_health / 10,
                                unit_type: bundle.combat_component.unit_type.clone(),
                                attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                attack_frequency: bundle.combat_component.attack_frequency,
                                attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                detection_range: bundle.combat_component.detection_range,
                                attack_range: bundle.combat_component.attack_range,
                                enemies: bundle.combat_component.enemies.clone(),
                                is_static: bundle.combat_component.is_static,
                                unit_data: (
                                    new_construction_tile,
                                    bundle.combat_component.unit_data.1.clone()
                                ),
                            })
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(0.),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                    ..default()
                                })
                                .insert(ConstructionProgressBar {
                                    constrcution_entity: client_entity,
                                    max_width: bar_size,
                                });
                            });
                        },
                        BuildingsBundles::VehicleFactory(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingConstructionSite{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_total: building.4,
                                build_power_remaining: building.4,
                                name: name.clone(),
                                build_distance: building.5,
                                current_builder: Entity::PLACEHOLDER,
                                resource_cost: building.6,
                            }).insert(CombatComponent{
                                team: team,
                                current_health: bundle.combat_component.current_health / 10,
                                max_health: bundle.combat_component.current_health / 10,
                                unit_type: bundle.combat_component.unit_type.clone(),
                                attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                attack_frequency: bundle.combat_component.attack_frequency,
                                attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                detection_range: bundle.combat_component.detection_range,
                                attack_range: bundle.combat_component.attack_range,
                                enemies: bundle.combat_component.enemies.clone(),
                                is_static: bundle.combat_component.is_static,
                                unit_data: (
                                    new_construction_tile,
                                    bundle.combat_component.unit_data.1.clone()
                                ),
                            })
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(0.),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                    ..default()
                                })
                                .insert(ConstructionProgressBar {
                                    constrcution_entity: client_entity,
                                    max_width: bar_size,
                                });
                            });
                        },
                        BuildingsBundles::LogisticHub(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingConstructionSite{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_total: building.4,
                                build_power_remaining: building.4,
                                name: name.clone(),
                                build_distance: building.5,
                                current_builder: Entity::PLACEHOLDER,
                                resource_cost: building.6,
                            }).insert(CombatComponent{
                                team: team,
                                current_health: bundle.combat_component.current_health / 10,
                                max_health: bundle.combat_component.current_health / 10,
                                unit_type: bundle.combat_component.unit_type.clone(),
                                attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                attack_frequency: bundle.combat_component.attack_frequency,
                                attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                detection_range: bundle.combat_component.detection_range,
                                attack_range: bundle.combat_component.attack_range,
                                enemies: bundle.combat_component.enemies.clone(),
                                is_static: bundle.combat_component.is_static,
                                unit_data: (
                                    new_construction_tile,
                                    bundle.combat_component.unit_data.1.clone()
                                ),
                            })
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(0.),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                    ..default()
                                })
                                .insert(ConstructionProgressBar {
                                    constrcution_entity: client_entity,
                                    max_width: bar_size,
                                });
                            });
                        },
                        BuildingsBundles::ResourceMiner(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingConstructionSite{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_total: building.4,
                                build_power_remaining: building.4,
                                name: name.clone(),
                                build_distance: building.5,
                                current_builder: Entity::PLACEHOLDER,
                                resource_cost: building.6,
                            }).insert(CombatComponent{
                                team: team,
                                current_health: bundle.combat_component.current_health / 10,
                                max_health: bundle.combat_component.current_health / 10,
                                unit_type: bundle.combat_component.unit_type.clone(),
                                attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                attack_frequency: bundle.combat_component.attack_frequency,
                                attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                detection_range: bundle.combat_component.detection_range,
                                attack_range: bundle.combat_component.attack_range,
                                enemies: bundle.combat_component.enemies.clone(),
                                is_static: bundle.combat_component.is_static,
                                unit_data: (
                                    new_construction_tile,
                                    bundle.combat_component.unit_data.1.clone()
                                ),
                            })
                            .id();

                            unit_type = bundle.combat_component.unit_type;

                            let mut nearest_zone = (f32::INFINITY, Entity::PLACEHOLDER);

                            for zone in queries.0.iter() {
                                let distance = zone.1.translation.distance(position);

                                if distance < nearest_zone.0 {
                                    nearest_zone.0 = distance;
                                    nearest_zone.1 = zone.0;
                                }
                            }

                            if let Ok(mut zone) = queries.0.get_mut(nearest_zone.1) {
                                zone.2.current_miners.entry(team).or_insert_with(|| None);
                                if let Some(miners) = zone.2.current_miners.get_mut(&team) {
                                    *miners = Some((client_entity, 1));
                                }
                            }

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(0.),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                    ..default()
                                })
                                .insert(ConstructionProgressBar {
                                    constrcution_entity: client_entity,
                                    max_width: bar_size,
                                });
                            });
                        },
                        BuildingsBundles::Pillbox(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingConstructionSite{
                                team: team,
                                building_bundle: building.1.clone(),
                                build_power_total: building.4,
                                build_power_remaining: building.4,
                                name: name.clone(),
                                build_distance: building.5,
                                current_builder: Entity::PLACEHOLDER,
                                resource_cost: building.6,
                            }).insert(CombatComponent{
                                team: team,
                                current_health: bundle.combat_component.current_health / 10,
                                max_health: bundle.combat_component.current_health / 10,
                                unit_type: bundle.combat_component.unit_type.clone(),
                                attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                attack_frequency: bundle.combat_component.attack_frequency,
                                attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                detection_range: bundle.combat_component.detection_range,
                                attack_range: bundle.combat_component.attack_range,
                                enemies: bundle.combat_component.enemies.clone(),
                                is_static: bundle.combat_component.is_static,
                                unit_data: (
                                    new_construction_tile,
                                    bundle.combat_component.unit_data.1.clone()
                                ),
                            })
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(0.),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                    ..default()
                                })
                                .insert(ConstructionProgressBar {
                                    constrcution_entity: client_entity,
                                    max_width: bar_size,
                                });
                            });
                        }
                        BuildingsBundles::None => {},
                    }

                    entity_maps.client_to_server.insert(client_entity, server_entity);
                    entity_maps.server_to_client.insert(server_entity, client_entity);
                }
            },
            ServerMessage::BuildingBuilt { team, construction_site_server_entity, server_entity, name, position, angle } => {
                if let Some(cs_entity) = entity_maps.server_to_client.clone().get(&construction_site_server_entity) {
                    commands.entity(*cs_entity).despawn();
                    entity_maps.client_to_server.remove(cs_entity);
                    entity_maps.server_to_client.remove(&construction_site_server_entity);
                }

                if let Some(building) = buildings_list.0.iter().find(|b| b.0 == name) {
                    let current_construction_site_tile = (
                        (position.x / TILE_SIZE) as i32,
                        (position.z / TILE_SIZE) as i32
                    );
    
                    let mut client_entity = Entity::PLACEHOLDER;
                    let mut unit_type = UnitTypes::None;

                    let color;
                    if team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let bar_size = resources.3.button_size * 0.75;
    
                    match &building.1 {
                        BuildingsBundles::InfantryBarracks(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                bundle.building_component.clone(),
                                CombatComponent{
                                    team: team,
                                    current_health: bundle.combat_component.current_health,
                                    max_health: bundle.combat_component.current_health,
                                    unit_type: bundle.combat_component.unit_type.clone(),
                                    attack_type: bundle.combat_component.attack_type.clone(),
                                    attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                    attack_frequency: bundle.combat_component.attack_frequency,
                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                    detection_range: bundle.combat_component.detection_range,
                                    attack_range: bundle.combat_component.attack_range,
                                    enemies: bundle.combat_component.enemies.clone(),
                                    is_static: bundle.combat_component.is_static,
                                    unit_data: (
                                        current_construction_site_tile,
                                        bundle.combat_component.unit_data.1.clone()
                                    ),
                                },
                                bundle.selectable.clone(),
                                bundle.producer.clone(),
                                bundle.human_resource_storage.clone(),
                                bundle.materials_storage.clone(),
                            )).insert(NavMeshAffector)
                            .insert(NavMeshAreaType(None))
                            .insert(DeconstructableBuilding{
                                team: team,
                                materials_spent: building.6,
                                buildpower_to_deconstruct_total: building.4,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: building.5,
                            })
                            .insert(SwitchableBuilding(true))
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(bar_size),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: HUMAN_RESOURCE_COLOR.into(),
                                    ..default()
                                })
                                .insert(HumanResourcesDisplay {
                                    original_width: bar_size,
                                    storage_entity: client_entity,
                                });
                            });

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(bar_size),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: MATERIALS_COLOR.into(),
                                    ..default()
                                })
                                .insert(MaterialsDisplay {
                                    original_width: bar_size,
                                    storage_entity: client_entity,
                                });
                            });
                        },
                        BuildingsBundles::VehicleFactory(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                bundle.building_component.clone(),
                                CombatComponent{
                                    team: team,
                                    current_health: bundle.combat_component.current_health,
                                    max_health: bundle.combat_component.current_health,
                                    unit_type: bundle.combat_component.unit_type.clone(),
                                    attack_type: bundle.combat_component.attack_type.clone(),
                                    attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                    attack_frequency: bundle.combat_component.attack_frequency,
                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                    detection_range: bundle.combat_component.detection_range,
                                    attack_range: bundle.combat_component.attack_range,
                                    enemies: bundle.combat_component.enemies.clone(),
                                    is_static: bundle.combat_component.is_static,
                                    unit_data: (
                                        current_construction_site_tile,
                                        bundle.combat_component.unit_data.1.clone()
                                    ),
                                },
                                bundle.selectable.clone(),
                                bundle.producer.clone(),
                                bundle.human_resource_storage.clone(),
                                bundle.materials_storage.clone(),
                            )).insert(NavMeshAffector)
                            .insert(NavMeshAreaType(None))
                            .insert(DeconstructableBuilding{
                                team: team,
                                materials_spent: building.6,
                                buildpower_to_deconstruct_total: building.4,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: building.5,
                            })
                            .insert(SwitchableBuilding(true))
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(bar_size),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: HUMAN_RESOURCE_COLOR.into(),
                                    ..default()
                                })
                                .insert(HumanResourcesDisplay {
                                    original_width: bar_size,
                                    storage_entity: client_entity,
                                });
                            });

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(bar_size),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: MATERIALS_COLOR.into(),
                                    ..default()
                                })
                                .insert(MaterialsDisplay {
                                    original_width: bar_size,
                                    storage_entity: client_entity,
                                });
                            });
                        },
                        BuildingsBundles::LogisticHub(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                SuppliesProductionComponent{
                                    supplies_storage_capacity: bundle.building_component.supplies_storage_capacity,
                                    available_supplies: bundle.building_component.available_supplies,
                                    supplies_production: bundle.building_component.supplies_production.clone(),
                                    production_local_point: bundle.building_component.production_local_point,
                                    elapsed_production_time: bundle.building_component.elapsed_production_time,
                                    supply_cooldown: bundle.building_component.supply_cooldown,
                                    elapsed_cooldown_time: bundle.building_component.elapsed_cooldown_time,
                                },
                                bundle.storage.clone(),
                                CombatComponent{
                                    team: team,
                                    current_health: bundle.combat_component.current_health,
                                    max_health: bundle.combat_component.current_health,
                                    unit_type: bundle.combat_component.unit_type.clone(),
                                    attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                    attack_frequency: bundle.combat_component.attack_frequency,
                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                    detection_range: bundle.combat_component.detection_range,
                                    attack_range: bundle.combat_component.attack_range,
                                    enemies: bundle.combat_component.enemies.clone(),
                                    is_static: bundle.combat_component.is_static,
                                    unit_data: (
                                        current_construction_site_tile,
                                        bundle.combat_component.unit_data.1.clone()
                                    ),
                                },
                            )).insert(NavMeshAffector)
                            .insert(NavMeshAreaType(None))
                            .insert(DeconstructableBuilding{
                                team: team,
                                materials_spent: building.6,
                                buildpower_to_deconstruct_total: building.4,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: building.5,
                            })
                            .insert(SwitchableBuilding(true))
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;

                            commands.spawn(NodeBundle{
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(bar_size),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                ..default()
                            })
                            .insert(Visibility::Hidden)
                            .with_children(|parent| {
                                parent.spawn(NodeBundle {
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(bar_size),
                                        height: Val::Px(bar_size / 4.),
                                        flex_direction: FlexDirection::Column,
                                        justify_content: JustifyContent::Start,
                                        align_items: AlignItems::Start,
                                        ..default()
                                    },
                                    background_color: MATERIALS_COLOR.into(),
                                    ..default()
                                })
                                .insert(MaterialsDisplay {
                                    original_width: bar_size,
                                    storage_entity: client_entity,
                                });
                            });
                        },
                        BuildingsBundles::ResourceMiner(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                bundle.building_component.clone(),
                                CombatComponent{
                                    team: team,
                                    current_health: bundle.combat_component.current_health,
                                    max_health: bundle.combat_component.current_health,
                                    unit_type: bundle.combat_component.unit_type.clone(),
                                    attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                    attack_frequency: bundle.combat_component.attack_frequency,
                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                    detection_range: bundle.combat_component.detection_range,
                                    attack_range: bundle.combat_component.attack_range,
                                    enemies: bundle.combat_component.enemies.clone(),
                                    is_static: bundle.combat_component.is_static,
                                    unit_data: (
                                        current_construction_site_tile,
                                        bundle.combat_component.unit_data.1.clone()
                                    ),
                                },
                            ))
                            .insert(NavMeshAffector)
                            .insert(NavMeshAreaType(None))
                            .insert(DeconstructableBuilding{
                                team: team,
                                materials_spent: building.6,
                                buildpower_to_deconstruct_total: building.4,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: building.5,
                            })
                            .id();

                            unit_type = bundle.combat_component.unit_type;

                            let mut nearest_zone = (f32::INFINITY, Entity::PLACEHOLDER);

                            for zone in queries.0.iter() {
                                let distance = zone.1.translation.distance(position);

                                if distance < nearest_zone.0 {
                                    nearest_zone.0 = distance;
                                    nearest_zone.1 = zone.0;
                                }
                            }

                            if let Ok(mut zone) = queries.0.get_mut(nearest_zone.1) {
                                zone.2.current_miners.entry(team).or_insert_with(|| None);
                                if let Some(miners) = zone.2.current_miners.get_mut(&team) {
                                    *miners = Some((client_entity, 1));
                                }
                            }
                        },
                        BuildingsBundles::Pillbox(bundle) => {
                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(bundle.model.mesh.id(), team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(bundle.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((bundle.model.mesh.id(), team), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                CoverComponent{
                                    cover_efficiency: bundle.building_component.cover_efficiency,
                                    points: bundle.building_component.points.clone(),
                                    units_inside: bundle.building_component.units_inside.clone(),
                                },
                                CombatComponent{
                                    team: team,
                                    current_health: bundle.combat_component.current_health,
                                    max_health: bundle.combat_component.current_health,
                                    unit_type: bundle.combat_component.unit_type.clone(),
                                    attack_type: bundle.combat_component.attack_type.clone(),
                                attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                    attack_frequency: bundle.combat_component.attack_frequency,
                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                    detection_range: bundle.combat_component.detection_range,
                                    attack_range: bundle.combat_component.attack_range,
                                    enemies: bundle.combat_component.enemies.clone(),
                                    is_static: bundle.combat_component.is_static,
                                    unit_data: (
                                        current_construction_site_tile,
                                        bundle.combat_component.unit_data.1.clone()
                                    ),
                                },
                            )).insert(NavMeshAffector)
                            .insert(NavMeshAreaType(None))
                            .insert(DeconstructableBuilding{
                                team: team,
                                materials_spent: building.6,
                                buildpower_to_deconstruct_total: building.4,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: building.5,
                            })
                            .id();
    
                            unit_type = bundle.combat_component.unit_type;
                        }
                        BuildingsBundles::None => {},
                    }

                    entity_maps.client_to_server.insert(client_entity, server_entity);
                    entity_maps.server_to_client.insert(server_entity, client_entity);
                }
            },
            ServerMessage::UnitSpawned { unit_data, position, server_entity } => {
                let tile = ((position.x / TILE_SIZE) as i32, (position.z / TILE_SIZE) as i32);

                if let Some(unit_production_data) = producable_units.barrack_producables.get(&unit_data.1.2) {
                    let mut client_entity= Entity::PLACEHOLDER;

                    let color;
                    let simplified_material;
                    if unit_data.0 == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                        simplified_material = materials.1.blue_solid.clone();
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                        simplified_material = materials.1.red_solid.clone();
                    }

                    let unit_type;

                    match &unit_production_data.0 {
                        UnitBundles::Soldier(b) => {
                            unit_type = b.combat_component.unit_type.clone();
                            
                            client_entity = commands.spawn((
                                SceneBundle{
                                    scene: b.scene.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                b.animation_component.clone(),
                                ChangeMaterial,
                                LOD{
                                    detailed: (Handle::default(), None, None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        },
                        UnitBundles::Shock(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            client_entity = commands.spawn((
                                SceneBundle{
                                    scene: b.scene.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        (
                                            unit_data.1.clone()
                                        ),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                b.animation_component.clone(),
                                ChangeMaterial,
                                LOD{
                                    detailed: (Handle::default(), None, None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        },
                        UnitBundles::Tank(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material_turret;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_turret.mesh.id(), unit_data.0)) {
                                material_turret = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_turret.material.id()) {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_turret.mesh.id(), unit_data.0), material_turret.clone());
                            }

                            let turret = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_turret.mesh.clone(),
                                    material: material_turret.clone(),
                                    ..default()
                                },
                                LOD{
                                    detailed: (b.model_turret.mesh.clone(), Some(material_turret.clone()), None),
                                    simplified: (b.lod.1.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();

                            let material_hull;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_hull.mesh.id(), unit_data.0)) {
                                material_hull = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_hull.material.id()) {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_hull.mesh.id(), unit_data.0), material_hull.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_hull.mesh.clone(),
                                    material: material_hull.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        (
                                            unit_data.1.clone()
                                        ),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                LOD{
                                    detailed: (b.model_hull.mesh.clone(), Some(material_hull.clone()), None),
                                    simplified: (b.lod.0.mesh.clone(), simplified_material.clone()),
                                },
                            )).push_children(&[turret]).id();
                        },
                        UnitBundles::IFV(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material_turret;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_turret.mesh.id(), unit_data.0)) {
                                material_turret = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_turret.material.id()) {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_turret.mesh.id(), unit_data.0), material_turret.clone());
                            }

                            let turret = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_turret.mesh.clone(),
                                    material: material_turret.clone(),
                                    ..default()
                                },
                                LOD{
                                    detailed: (b.model_turret.mesh.clone(), Some(material_turret.clone()), None),
                                    simplified: (b.lod.1.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();

                            let material_hull;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_hull.mesh.id(), unit_data.0)) {
                                material_hull = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_hull.material.id()) {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_hull.mesh.id(), unit_data.0), material_hull.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_hull.mesh.clone(),
                                    material: material_hull.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        (
                                            unit_data.1.clone()
                                        ),
                                    ),
                                },
                                b.transport.clone(),
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                LOD{
                                    detailed: (b.model_hull.mesh.clone(), Some(material_hull.clone()), None),
                                    simplified: (b.lod.0.mesh.clone(), simplified_material.clone()),
                                },
                            )).push_children(&[turret]).id();
                        },
                        _ => { unit_type = UnitTypes::None; },
                    }

                    match unit_data.1.0 {
                        CompanyTypes::Regular => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().regular_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if unit_data.1.1.5 == 0 {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(client_entity);
                                    }
                                } else {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(client_entity);
                                    }
                                }
                            }
                        },
                        CompanyTypes::Shock => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().shock_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if unit_data.1.1.5 == 0 {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(client_entity);
                                    }
                                } else {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(client_entity);
                                    }
                                }
                            }
                        },
                        CompanyTypes::Armored => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().armored_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if client_entity != Entity::PLACEHOLDER {
                                    let _ = platoon.0.0.insert(client_entity);
                                }
                            }
                        },
                        CompanyTypes::Artillery => {
                            if let Some(artillery_unit) =
                            armies.0.get_mut(&unit_data.0).unwrap().artillery_units.0.get_mut(&unit_data.1.1.6){
                                if client_entity != Entity::PLACEHOLDER {
                                    artillery_unit.0.0 = Some(client_entity);
                                }
                            }
                        },
                        CompanyTypes::Engineer => {
                            if let Some(engineer) =
                            armies.0.get_mut(&unit_data.0).unwrap().engineers.get_mut(&unit_data.1.1.6){
                                if client_entity != Entity::PLACEHOLDER {
                                    engineer.0.0 = Some(client_entity);
                                }
                            }
                        },
                        CompanyTypes::None => {},
                    }

                    tile_map.tiles.entry(unit_data.0).or_insert_with(HashMap::new).entry(tile)
                    .or_insert_with(HashMap::new).insert(client_entity, (position, unit_type));

                    entity_maps.client_to_server.insert(client_entity, server_entity);
                    entity_maps.server_to_client.insert(server_entity, client_entity);
                }
                else if let Some(unit_production_data) = producable_units.factory_producables.get(&unit_data.1.2) {
                    let mut client_entity = Entity::PLACEHOLDER;

                    let color;
                    let simplified_material;
                    if unit_data.0 == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                        simplified_material = materials.1.blue_solid.clone();
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                        simplified_material = materials.1.red_solid.clone();
                    }

                    let unit_type;

                    match &unit_production_data.0 {
                        UnitBundles::Soldier(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            client_entity = commands.spawn((
                                SceneBundle{
                                    scene: b.scene.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                b.animation_component.clone(),
                                ChangeMaterial,
                                LOD{
                                    detailed: (Handle::default(), None, None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        },
                        UnitBundles::Shock(b) => {
                            unit_type = b.combat_component.unit_type.clone();
                            
                            client_entity = commands.spawn((
                                SceneBundle{
                                    scene: b.scene.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                b.animation_component.clone(),
                                ChangeMaterial,
                                LOD{
                                    detailed: (Handle::default(), None, None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        },
                        UnitBundles::Tank(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material_turret;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_turret.mesh.id(), unit_data.0)) {
                                material_turret = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_turret.material.id()) {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_turret.mesh.id(), unit_data.0), material_turret.clone());
                            }

                            let turret = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_turret.mesh.clone(),
                                    material: material_turret.clone(),
                                    ..default()
                                },
                                LOD{
                                    detailed: (b.model_turret.mesh.clone(), Some(material_turret.clone()), None),
                                    simplified: (b.lod.1.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();

                            let material_hull;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_hull.mesh.id(), unit_data.0)) {
                                material_hull = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_hull.material.id()) {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_hull.mesh.id(), unit_data.0), material_hull.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_hull.mesh.clone(),
                                    material: material_hull.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                LOD{
                                    detailed: (b.model_hull.mesh.clone(), Some(material_hull.clone()), None),
                                    simplified: (b.lod.0.mesh.clone(), simplified_material.clone()),
                                },
                            )).push_children(&[turret]).id();
                        },
                        UnitBundles::IFV(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material_turret;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_turret.mesh.id(), unit_data.0)) {
                                material_turret = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_turret.material.id()) {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_turret = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_turret.mesh.id(), unit_data.0), material_turret.clone());
                            }

                            let turret = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_turret.mesh.clone(),
                                    material: material_turret.clone(),
                                    ..default()
                                },
                                LOD{
                                    detailed: (b.model_turret.mesh.clone(), Some(material_turret.clone()), None),
                                    simplified: (b.lod.1.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();

                            let material_hull;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model_hull.mesh.id(), unit_data.0)) {
                                material_hull = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model_hull.material.id()) {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material_hull = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model_hull.mesh.id(), unit_data.0), material_hull.clone());
                            }
                            
                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model_hull.mesh.clone(),
                                    material: material_hull.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.transport.clone(),
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                LOD{
                                    detailed: (b.model_hull.mesh.clone(), Some(material_hull.clone()), None),
                                    simplified: (b.lod.0.mesh.clone(), simplified_material.clone()),
                                },
                            )).push_children(&[turret]).id();
                        },
                        UnitBundles::Artillery(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model.mesh.id(), unit_data.0)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model.mesh.id(), unit_data.0), material.clone());
                            }

                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.artillery_component.clone(),
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                SelectableUnit,
                                LOD{
                                    detailed: (b.model.mesh.clone(), Some(material.clone()), None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        },
                        UnitBundles::Engineer(b) => {
                            unit_type = b.combat_component.unit_type.clone();

                            let material;

                            if let Some(mat) = materials.1.team_materials.get(&(b.model.mesh.id(), unit_data.0)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.0.get(b.model.material.id()) {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials.1.team_materials.insert((b.model.mesh.id(), unit_data.0), material.clone());
                            }
                            
                            client_entity = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: b.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: Transform::from_translation(position),
                                    ..default()
                                },
                                b.unit_component.clone(),
                                CombatComponent {
                                    team: unit_data.0,
                                    current_health: b.combat_component.current_health,
                                    max_health: b.combat_component.max_health,
                                    unit_type: b.combat_component.unit_type.clone(),
                                    attack_type: b.combat_component.attack_type.clone(),
                                    attack_animation_type: b.combat_component.attack_animation_type.clone(),
                                    attack_frequency: b.combat_component.attack_frequency,
                                    attack_elapsed_time: b.combat_component.attack_elapsed_time,
                                    detection_range: b.combat_component.detection_range,
                                    attack_range: b.combat_component.attack_range,
                                    enemies: b.combat_component.enemies.clone(),
                                    is_static: b.combat_component.is_static,
                                    unit_data: (
                                        tile,
                                        unit_data.1.clone(),
                                    ),
                                },
                                b.engineer_component.clone(),
                                b.supplies_consumer.clone(),
                                b.controller.clone(),
                                LOD{
                                    detailed: (b.model.mesh.clone(), Some(material.clone()), None),
                                    simplified: (b.lod.mesh.clone(), simplified_material.clone()),
                                },
                            )).id();
                        }
                    }
    
                    match unit_data.1.0 {
                        CompanyTypes::Regular => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().regular_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if unit_data.1.1.5 == 0 {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(client_entity);
                                    }
                                } else {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(client_entity);
                                    }
                                }
                            }
                        },
                        CompanyTypes::Shock => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().shock_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if unit_data.1.1.5 == 0 {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(client_entity);
                                    }
                                } else {
                                    if client_entity != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(client_entity);
                                    }
                                }
                            }
                        },
                        CompanyTypes::Armored => {
                            if let Some (platoon) = armies.0.get_mut(&unit_data.0).unwrap().armored_squads.get_mut(&(
                                unit_data.1.1.0,
                                unit_data.1.1.1,
                                unit_data.1.1.2,
                                unit_data.1.1.3,
                                unit_data.1.1.4,
                            )) {
                                if client_entity != Entity::PLACEHOLDER {
                                    let _ = platoon.0.0.insert(client_entity);
                                }
                            }
                        },
                        CompanyTypes::Artillery => {
                            if let Some(artillery_unit) =
                            armies.0.get_mut(&unit_data.0).unwrap().artillery_units.0.get_mut(&unit_data.1.1.6){
                                if client_entity != Entity::PLACEHOLDER {
                                    artillery_unit.0.0 = Some(client_entity);
                                }
                            }
                        },
                        CompanyTypes::Engineer => {
                            if let Some(engineer) =
                            armies.0.get_mut(&unit_data.0).unwrap().engineers.get_mut(&unit_data.1.1.6){
                                if client_entity != Entity::PLACEHOLDER {
                                    engineer.0.0 = Some(client_entity);
                                }
                            }
                        },
                        CompanyTypes::None => {},
                    }

                    tile_map.tiles.entry(unit_data.0).or_insert_with(HashMap::new).entry(tile)
                    .or_insert_with(HashMap::new).insert(client_entity, (position, unit_type));

                    entity_maps.client_to_server.insert(client_entity, server_entity);
                    entity_maps.server_to_client.insert(server_entity, client_entity);
                }
            },
            ServerMessage::UnitPathInserted { server_entity, path } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity){
                    entities_to_move.1.0.push((*client_entity, path));
                }
            },
            ServerMessage::UnitDamaged { server_entity, damage } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity){
                    //units_to_damage.0.push((*client_entity, damage));
                }
            },
            ServerMessage::UnitRemoved { server_entity, unit_data, should_spawn_corpse } => {
                if let Some(army) = armies.0.get_mut(&unit_data.0) {
                    if let Some(client_entity) = entity_maps.server_to_client.clone().get(&server_entity) {
                        if let Ok(transform) = queries.1.get(*client_entity) {
                            if should_spawn_corpse {
                                let mut mesh: Handle<Mesh> = Handle::default();
                                let mut material: Handle<StandardMaterial> = Handle::default();
                                let mut remains_type = "unspecified";

                                match unit_data.2.2.as_str() {
                                    "regular_soldier" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "atgm" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "shock_soldier" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "lat" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "sniperr" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "snipers" => {
                                        mesh = resources.2.corpse.0.clone();
                                        material = resources.2.corpse.1.clone();
                                        remains_type = "infantry";
                                    }
                                    "tank" => {
                                        mesh = resources.2.tank.0.clone();
                                        material = materials.1.wreck_material.clone();
                                        remains_type = "vehicle";
                                    }
                                    "ifv" => {
                                        mesh = resources.2.ifv.0.clone();
                                        material = materials.1.wreck_material.clone();
                                        remains_type = "vehicle";
                                    }
                                    "artillery" => {
                                        mesh = resources.2.artillery.0.clone();
                                        material = materials.1.wreck_material.clone();
                                        remains_type = "vehicle";
                                    }
                                    "engineer" => {
                                        mesh = resources.2.engineer.0.clone();
                                        material = materials.1.wreck_material.clone();
                                        remains_type = "vehicle";
                                    }
                                    _ => {}
                                }

                                resources.4.0 += 1;

                                match remains_type {
                                    "infantry" => {
                                        let color;
                                        let simplified_material;
                                        if unit_data.0 == 1 {
                                            color = Vec4::new(0., 0., 1., 1.);
                                            simplified_material = materials.1.blue_solid.clone();
                                        } else {
                                            color = Vec4::new(1., 0., 0., 1.);
                                            simplified_material = materials.1.red_solid.clone();
                                        }

                                        let team_material;

                                        if let Some(mat) = materials.1.team_materials.get(&(mesh.id(), unit_data.0)) {
                                            team_material = mat.clone();
                                        } else {
                                            if let Some(original) = materials.0.get(material.id()) {
                                                team_material = materials.2.add(ExtendedMaterial {
                                                    base: original.clone(),
                                                    extension: TeamMaterialExtension {
                                                        team_color: color,
                                                    },
                                                });
                                            } else {
                                                team_material = materials.2.add(ExtendedMaterial {
                                                    base: StandardMaterial{
                                                        ..default()
                                                    },
                                                    extension: TeamMaterialExtension {
                                                        team_color: color,
                                                    },
                                                });
                                            }

                                            materials.1.team_materials.insert((mesh.id(), unit_data.0), team_material.clone());
                                        }

                                        commands.spawn(MaterialMeshBundle{
                                            mesh: mesh.clone(),
                                            material: team_material.clone(),
                                            transform: *transform,
                                            ..default()
                                        })
                                        .insert(UnitRemains{
                                            number: resources.4.0,
                                        }).insert(LOD{
                                            detailed: (mesh, Some(team_material), None),
                                            simplified: (resources.2.corpse_simplified_mesh.clone(), simplified_material),
                                        });
                                    }
                                    "vehicle" => {
                                        commands.spawn(MaterialMeshBundle{
                                            mesh: mesh.clone(),
                                            material: material.clone(),
                                            transform: *transform,
                                            ..default()
                                        })
                                        .insert(UnitRemains{
                                            number: resources.4.0,
                                        }).insert(LOD{
                                            detailed: (mesh, None, Some(material.clone())),
                                            simplified: (resources.2.vehicle_simplified_mesh.clone(), material),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }

                        match unit_data.2.0 {
                            CompanyTypes::Regular => {
                                if let Some(platoon) = army.regular_squads.get_mut(&(
                                    unit_data.2.1.0,
                                    unit_data.2.1.1,
                                    unit_data.2.1.2,
                                    unit_data.2.1.3,
                                    unit_data.2.1.4,
                                )){
                                    if unit_data.2.1.5 == 0 {
                                        platoon.0.0.0.remove(client_entity);
                                    } else {
                                        platoon.0.0.1.remove(client_entity);
                                    }
                                }
                            },
                            CompanyTypes::Shock => {
                                if let Some(platoon) = army.shock_squads.get_mut(&(
                                    unit_data.2.1.0,
                                    unit_data.2.1.1,
                                    unit_data.2.1.2,
                                    unit_data.2.1.3,
                                    unit_data.2.1.4,
                                )){
                                    if unit_data.2.1.5 == 0 {
                                        platoon.0.0.0.remove(client_entity);
                                    } else {
                                        platoon.0.0.1.remove(client_entity);
                                    }
                                }
                            },
                            CompanyTypes::Armored => {
                                if let Some(platoon) = army.armored_squads.get_mut(&(
                                    unit_data.2.1.0,
                                    unit_data.2.1.1,
                                    unit_data.2.1.2,
                                    unit_data.2.1.3,
                                    unit_data.2.1.4,
                                )){
                                    platoon.0.0.remove(client_entity);
                                }
                            },
                            CompanyTypes::Artillery => {
                                army.artillery_units.0.remove(&unit_data.2.1.6);
                            },
                            CompanyTypes::Engineer => {
                                army.engineers.remove(&unit_data.2.1.6);
                            },
                            CompanyTypes::None => {},
                        }

                        tile_map.tiles.entry(unit_data.0).or_insert_with(HashMap::new).entry(unit_data.1)
                        .or_insert_with(HashMap::new).remove(client_entity);

                        commands.entity(*client_entity).despawn_recursive();

                        entity_maps.client_to_server.remove(client_entity);
                        entity_maps.server_to_client.remove(&server_entity);
                    }
                }
            },
            ServerMessage::ArtilleryProjectileSpawned { position, server_entity } => {
                let client_entity = commands.spawn(MaterialMeshBundle {
                    mesh: resources.1.shell.0.clone(),
                    material: resources.1.shell.1.clone(),
                    transform: Transform::from_translation(position),
                    ..default()
                })
                .insert(TrailEmmiterComponent)
                .id();

                let mesh_handle = meshes.add(Triangle3d{
                    vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
                });

                commands.spawn(MaterialMeshBundle{
                    mesh: mesh_handle.clone(),
                    material: materials.1.red_solid.clone(),
                    transform: Transform::from_translation(position),
                    ..default()
                })
                .insert(
                    TrailComponent{
                        positions: vec![],
                        length: 10,
                        width: 0.05,
                        mesh_handle,
                        emmiter_entity: client_entity,
                    }
                );

                sounds_counter += 1;

                if sounds_counter <= 3 {
                    commands.entity(client_entity).try_insert(
                        AudioBundle{
                            source: resources.1.tank_shot_sound.clone(),
                            settings: PlaybackSettings{
                                mode: PlaybackMode::Remove,
                                volume: Volume::new(100.),
                                speed: 1.,
                                paused: false,
                                spatial: true,
                                spatial_scale: None,
                            },
                        }
                    );
                }

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::HomingProjectileSpawned { position, server_entity } => {
                let client_entity = commands.spawn(MaterialMeshBundle {
                    mesh: resources.1.shell.0.clone(),
                    material: resources.1.shell.1.clone(),
                    transform: Transform::from_translation(position),
                    ..default()
                })
                .insert(TrailEmmiterComponent)
                .id();

                let mesh_handle = meshes.add(Triangle3d{
                    vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
                });

                commands.spawn(MaterialMeshBundle{
                    mesh: mesh_handle.clone(),
                    material: materials.1.red_solid.clone(),
                    transform: Transform::from_translation(position),
                    ..default()
                })
                .insert(
                    TrailComponent{
                        positions: vec![],
                        length: 10,
                        width: 0.05,
                        mesh_handle,
                        emmiter_entity: client_entity,
                    }
                );

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::LogisticUnitSpawned { position, server_entity, team} => {
                let color;
                let simplified_material;
                if team == 1 {
                    color = Vec4::new(0., 0., 1., 1.);
                    simplified_material = materials.1.blue_solid.clone();
                } else {
                    color = Vec4::new(1., 0., 0., 1.);
                    simplified_material = materials.1.red_solid.clone();
                }

                let material;

                if let Some(mat) = materials.1.team_materials.get(&(resources.2.truck.0.id(), team)) {
                    material = mat.clone();
                } else {
                    if let Some(original) = materials.0.get(resources.2.truck.1.id()) {
                        material = materials.2.add(ExtendedMaterial {
                            base: original.clone(),
                            extension: TeamMaterialExtension {
                                team_color: color,
                            },
                        });
                    } else {
                        material = materials.2.add(ExtendedMaterial {
                            base: StandardMaterial{
                                ..default()
                            },
                            extension: TeamMaterialExtension {
                                team_color: color,
                            },
                        });
                    }

                    materials.1.team_materials.insert((resources.2.truck.0.id(), team), material.clone());
                }
                
                let client_entity = commands.spawn(MaterialMeshBundle {
                    mesh: resources.2.truck.0.clone(),
                    material: material.clone(),
                    transform: Transform::from_translation(position),
                    ..default()
                })
                .insert(UnitComponent{
                    path: Vec::new(),
                    start_position: Vec3::ZERO,
                    quantized_destination: None,
                    speed: LOGISTIC_UNITS_SPEED,
                    waypoint_radius: 1.,
                    elapsed: 0.,
                    inv_duration: 0.,
                    last_position: Vec3::ZERO,
                    stuck_count: 0,
                })
                .insert(CombatComponent{
                    team: team,
                    current_health: 100,
                    max_health: 100,
                    unit_type: UnitTypes::LightVehicle,
                    attack_type: AttackTypes::None,
                    attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                    attack_frequency: 0,
                    attack_elapsed_time: 0,
                    enemies: Vec::new(),
                    detection_range: 0.,
                    attack_range: 0.,
                    is_static: false,
                    unit_data: (
                        (0, 0),
                        (
                            CompanyTypes::None,
                            (-1, -1, -1, -1, -1, -1, -1),
                            "".to_string(),
                        ),
                    ),
                })
                .insert(KinematicCharacterController{
                    custom_shape: Some((Collider::cuboid(0.5, 0.5, 0.5), Vec3::new(0., 0.5, 0.), Quat::IDENTITY)),
                    up: Vec3::Y,
                    offset: CharacterLength::Absolute(0.1),
                    slide: true,
                    autostep: None,
                    apply_impulse_to_dynamic_bodies: false,
                    snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                    ..default()
                })
                .insert(LOD{
                    detailed: (resources.2.truck.0.clone(), Some(material.clone()), None),
                    simplified: (resources.2.truck.2.clone(), simplified_material.clone()),
                })
                .id();

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::UnspecifiedEntityMoved { server_entity, new_position } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity){
                    entities_to_move.0.0.push((*client_entity, new_position));
                }
            },
            ServerMessage::UnspecifiedEntityRemoved { server_entity } => {
                if let Some(client_entity) = entity_maps.server_to_client.clone().get(&server_entity) {
                    commands.entity(*client_entity).despawn_recursive();

                    entity_maps.client_to_server.remove(client_entity);
                    entity_maps.server_to_client.remove(&server_entity);
                }
            },
            ServerMessage::SettlementCaptured { server_entity, team, captured_apartments } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut settlement) = queries.8.get_mut(*client_entity) {
                        if let Some(team_settlements) = resources.5.0.get_mut(&settlement.0.team) {
                            *team_settlements -= 1;

                            if *team_settlements == 0 {
                                other.2.set(GameState::GameEnd);
                            }
                        }

                        if let Some(team_settlements) = resources.5.0.get_mut(&team) {
                            *team_settlements += 1;
                        }

                        settlement.0.team = team;
                        settlement.0.elapsed_capture_time = 0;
                    }

                    for captured_apartment_server_entity in captured_apartments.iter() {
                        if let Some(apartment_client_entity) = entity_maps.server_to_client.get(captured_apartment_server_entity) {
                            if let Ok(mut apartment) = queries.11.get_mut(*apartment_client_entity) {
                                apartment.team = team;
                            }

                            commands.entity(*apartment_client_entity).insert(Visibility::Visible);
                        }
                    }

                    let color;
                    if team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let material;

                    if let Some(mat) = materials.1.team_materials.get(&(resources.0.town_hall.0.id(), team)) {
                        material = mat.clone();
                    } else {
                        if let Some(original) = materials.0.get(resources.0.town_hall.1.id()) {
                            material = materials.2.add(ExtendedMaterial {
                                base: original.clone(),
                                extension: TeamMaterialExtension {
                                    team_color: color,
                                },
                            });
                        } else {
                            material = materials.2.add(ExtendedMaterial {
                                base: StandardMaterial{
                                    ..default()
                                },
                                extension: TeamMaterialExtension {
                                    team_color: color,
                                },
                            });
                        }

                        materials.1.team_materials.insert((resources.0.town_hall.0.id(), team), material.clone());
                    }

                    commands.entity(*client_entity).insert(material);
                }
            },
            ServerMessage::ExplosionOccured { position } => {
                event_writer.3.send(ExplosionEvent((position, (0, DamageTypes::Universal), (0., 0, DamageTypes::Universal))));
            },
            ServerMessage::GameInitialized => {
                event_writer.0.send(ClientGameInitializedEvent);
            },
            ServerMessage::AllSettlementsPlaced => {
                event_writer.1.send(AllPlayersPlacedSettlementsEvent);
            },
            ServerMessage::GameStarted => {
                event_writer.2.send(ClientGameStartedEvent);
            },
            ServerMessage::UnitCovered { server_entity, initial_unit_position_y } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    commands.entity(*client_entity).insert(Covered{
                        cover_efficiency: 0.,
                        cover_entity: Entity::PLACEHOLDER,
                        original_y: initial_unit_position_y,
                    });
                }
            },
            ServerMessage::UnitUncovered { server_entity, original_y } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut transform) = queries.1.get_mut(*client_entity) {
                        transform.translation.y = original_y;

                        commands.entity(*client_entity).remove::<MovingToCover>();
                        commands.entity(*client_entity).remove::<Covered>();
                    }
                }
            },
            ServerMessage::UnitsSupplied { server_entities } => {
                for server_entity in server_entities.iter() {
                    if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity.0) {
                        if let Ok(mut consumer) = queries.2.get_mut(*client_entity) {
                            consumer.supplies += server_entity.1;

                            if consumer.supplies > consumer.supplies_capacity {
                                consumer.supplies = consumer.supplies_capacity;
                            }
                        }
                    }
                }
            },
            ServerMessage::MaterialsDelivered { server_entity, amount } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut storage) = queries.3.get_mut(*client_entity) {
                        storage.available_resources += amount;

                        if storage.available_resources > storage.materials_storage_capacity {
                            storage.available_resources = storage.materials_storage_capacity;
                        }
                    }
                }
            },
            ServerMessage::HumanResourcesDelivered { server_entity, amount } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut storage) = queries.4.get_mut(*client_entity) {
                        storage.available_human_resources += amount;

                        if storage.available_human_resources > storage.human_resource_storage_capacity {
                            storage.available_human_resources = storage.human_resource_storage_capacity;
                        }
                    }
                }
            },
            ServerMessage::ConstructionProgressChanged { server_entity, current_build_power } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut construction_site) = queries.5.get_mut(*client_entity) {
                        construction_site.build_power_remaining = current_build_power;
                    }
                }
            },
            ServerMessage::DeconstructionProgressChanged { server_entity, current_build_power } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    if let Ok(mut deconstruction_site) = queries.6.get_mut(*client_entity) {
                        deconstruction_site.buildpower_to_deconstruct_remaining = current_build_power;
                    }
                }
            },
            ServerMessage::DeconstructionAssigned { server_entity, team, deconstruction_distance } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    commands.entity(*client_entity).insert(ToDeconstruct{
                        team: team,
                        deconstructor_entity: Entity::PLACEHOLDER,
                        progress_bar_entity: Entity::PLACEHOLDER,
                        deconstruction_distance: deconstruction_distance,
                    });
                }
            },
            ServerMessage::DeconstructionStarted { server_deconstruction_entity } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_deconstruction_entity) {
                    if let Ok(mut deconstruction_site) = queries.7.get_mut(*client_entity) {
                        let bar_size = resources.3.button_size * 0.75;

                        let bar = commands.spawn(NodeBundle{
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(bar_size),
                                height: Val::Px(bar_size / 4.),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Start,
                                align_items: AlignItems::Start,
                                top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        })
                        .insert(Visibility::Hidden)
                        .with_children(|parent| {
                            parent.spawn(NodeBundle {
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(0.),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    ..default()
                                },
                                background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                                ..default()
                            })
                            .insert(ConstructionProgressBar {
                                constrcution_entity: *client_entity,
                                max_width: bar_size,
                            });
                        }).id();

                        deconstruction_site.progress_bar_entity = bar;
                    }
                }
            },
            ServerMessage::DeconstructionCanceled { server_entity } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                    commands.entity(*client_entity).remove::<ToDeconstruct>();

                    if let Ok(mut deconstruction_site) = queries.6.get_mut(*client_entity) {
                        deconstruction_site.buildpower_to_deconstruct_remaining = 0;
                    }
                }
            },
            ServerMessage::ApartmentConstructionSitePlaced { server_entity, position, angle, team } => {
                let new_construction_tile = ((position.x / TILE_SIZE) as i32, (position.z / TILE_SIZE) as i32);

                let client_entity = commands.spawn(MaterialMeshBundle{
                    mesh: resources.0.apartment.0.clone(),
                    material: materials.1.blue_transparent.clone(),
                    transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                    ..default()
                })
                .insert(BuildingConstructionSite{
                    team: team,
                    building_bundle: BuildingsBundles::None,
                    build_power_total: 200,
                    build_power_remaining: 200,
                    name: "".to_string(),
                    build_distance: 30.,
                    current_builder: Entity::PLACEHOLDER,
                    resource_cost: 0,
                }).insert(CombatComponent{
                    team: team,
                    current_health: 10,
                    max_health: 10,
                    unit_type: UnitTypes::Building,
                    attack_type: AttackTypes::None,
                    attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                    attack_frequency: 0,
                    attack_elapsed_time: 0,
                    detection_range: 0.,
                    attack_range: 0.,
                    enemies: vec![],
                    is_static: true,
                    unit_data: (
                        new_construction_tile,
                        (
                            CompanyTypes::None,
                            (0, 0, 0, 0, 0, 0, 0),
                            "".to_string(),
                        )
                    ),
                })
                .insert(DontTouch)
                .id();

                tile_map.tiles.entry(team).or_insert_with(HashMap::new).entry(new_construction_tile)
                .or_insert_with(HashMap::new).insert(client_entity, (position, UnitTypes::None));

                let bar_size = resources.3.button_size * 0.75;

                commands.spawn(NodeBundle{
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(bar_size),
                        height: Val::Px(bar_size / 4.),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Start,
                        align_items: AlignItems::Start,
                        top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                })
                .insert(Visibility::Hidden)
                .with_children(|parent| {
                    parent.spawn(NodeBundle {
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(0.),
                            height: Val::Px(bar_size / 4.),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Start,
                            align_items: AlignItems::Start,
                            ..default()
                        },
                        background_color: CONSTRUCTION_PROGRESS_COLOR.into(),
                        ..default()
                    })
                    .insert(ConstructionProgressBar {
                        constrcution_entity: client_entity,
                        max_width: bar_size,
                    });
                });

                entity_maps.client_to_server.insert(client_entity, server_entity);
                entity_maps.server_to_client.insert(server_entity, client_entity);
            },
            ServerMessage::SettlementCaptureStarted { settlement_server_entity } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&settlement_server_entity) {
                    if let Ok(mut settlement) = queries.8.get_mut(*client_entity) {
                        commands.entity(*client_entity).insert(SettlementCaptureInProgress);

                        let bar_size = resources.3.button_size * 0.75;

                        let color;
                        if settlement.0.team == 1 {
                            color = Color::srgba(1., 0., 0., 1.);
                        } else {
                            color = Color::srgba(0., 0., 1., 1.);
                        }

                        commands.spawn(NodeBundle{
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(bar_size),
                                height: Val::Px(bar_size / 4.),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Start,
                                align_items: AlignItems::Start,
                                top: Val::Px(bar_size / 2. + bar_size / 4. / 2.),
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        })
                        .insert(Visibility::Hidden)
                        .with_children(|parent| {
                            parent.spawn(NodeBundle {
                                style: Style {
                                    position_type: PositionType::Relative,
                                    width: Val::Px(0.),
                                    height: Val::Px(bar_size / 4.),
                                    flex_direction: FlexDirection::Column,
                                    justify_content: JustifyContent::Start,
                                    align_items: AlignItems::Start,
                                    ..default()
                                },
                                background_color: color.into(),
                                ..default()
                            })
                            .insert(SettlementCaptureProgressBar{
                                constrcution_entity: *client_entity,
                                max_width: bar_size,
                            });
                        });

                        settlement.0.elapsed_capture_time += 1000;
                    }
                }
            },
            ServerMessage::SettlementCaptureEnded { settlement_server_entity } => {
                if let Some(client_entity) = entity_maps.server_to_client.get(&settlement_server_entity) {
                    commands.entity(*client_entity).remove::<SettlementCaptureInProgress>();

                    if let Ok(mut settlement) = queries.8.get_mut(*client_entity) {
                        settlement.0.elapsed_capture_time = 0;
                    }
                }
            },
            ServerMessage::ResourceDisplayesUpdated { materials_display, human_resource_display } => {
                if other.1.team == 1 {
                    for mut material_display in queries.9.iter_mut() {
                        material_display.sections[0].value = materials_display.0.clone();
                    }

                    for mut human_resources_display in queries.10.iter_mut() {
                        human_resources_display.sections[0].value = human_resource_display.0.clone();
                    }
                } else {
                    for mut material_display in queries.9.iter_mut() {
                        material_display.sections[0].value = materials_display.1.clone();
                    }

                    for mut human_resources_display in queries.10.iter_mut() {
                        human_resources_display.sections[0].value = human_resource_display.1.clone();
                    }
                }
            },
            ServerMessage::TransportAssignationCanceled { server_entities } => {
                for server_entity in server_entities.iter() {
                    if let Some(client_entity) = entity_maps.server_to_client.get(server_entity) {
                        commands.entity(*client_entity).remove::<MovingToTransport>();
                        commands.entity(*client_entity).remove::<NeedToMove>();
                    }
                }
            },
            ServerMessage::UnitsEmbarked { server_entities, transport_server_entity, team } => {
                if let Some(client_transport_entity) = entity_maps.server_to_client.get(&transport_server_entity) {
                    if let Ok(mut transport) = queries.12.get_mut(*client_transport_entity) {
                        for server_unit_entity in server_entities.iter() {
                            if let Some(client_unit_entity) = entity_maps.server_to_client.get(&server_unit_entity.0) {
                                if let Ok(mut unit_transform) = queries.1.get_mut(*client_unit_entity) {
                                    commands.entity(*client_unit_entity).remove::<MovingToTransport>();
                                    commands.entity(*client_unit_entity).remove::<NeedToMove>();
                                    commands.entity(*client_unit_entity).insert(DisabledUnit);
                                    commands.entity(*client_unit_entity).insert(InTransport{
                                        transport_entity: *client_transport_entity,
                                    });

                                    if let Some(team_map) = tile_map.tiles.get_mut(&team) {
                                        if let Some(tile) = team_map.get_mut(&server_unit_entity.1) {
                                            tile.remove(client_unit_entity);
                                        }
                                    }

                                    unit_transform.translation = Vec3::new(0., 10000., 0.);

                                    transport.units_inside.insert(*client_unit_entity);
                                }
                            }
                        }
                    }
                }
            },
            ServerMessage::UnitsDisembarked { server_entities, transport_server_entity, transport_position } => {
                if let Some(client_transport_entity) = entity_maps.server_to_client.get(&transport_server_entity) {
                    if let Ok(mut transport) = queries.12.get_mut(*client_transport_entity){
                        for server_entity in server_entities.iter() {
                            if let Some(client_unit_entity) = entity_maps.server_to_client.get(server_entity) {
                                if let Ok(mut unit) = queries.1.get_mut(*client_unit_entity) {
                                    commands.entity(*client_unit_entity).remove::<DisabledUnit>();
                                    commands.entity(*client_unit_entity).remove::<InTransport>();

                                    unit.translation = transport_position + Vec3::new(0., 0., 0.);
                                }
                            }
                        }

                        transport.units_inside.clear();
                    }
                }
            },
            ServerMessage::HostArmyChanged { army } => {
                let mut regular_platoons: HashMap<(i32, i32, i32, i32, i32), (RegularSquad, String, Entity)> = HashMap::new();
                let mut shock_platoons: HashMap<(i32, i32, i32, i32, i32), (ShockSquad, String, Entity)> = HashMap::new();
                let mut armored_platoons: HashMap<(i32, i32, i32, i32, i32), (ArmoredSquad, String, Entity)> = HashMap::new();
                let mut artillery_units: (HashMap<i32, ((Option<Entity>, String), Entity)>, Entity) = (HashMap::new(), Entity::PLACEHOLDER);
                let mut engineers: HashMap<i32, ((Option<Entity>, String), Entity)> = HashMap::new();

                for s_regular_platoon in army.regular_platoons.iter() {
                    let mut soldiers: LimitedHashSet<Entity, REGULAR_SQUAD_SIZE> = LimitedHashSet::new();
                    let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_REGULAR_SQUAD> = LimitedHashSet::new();

                    for soldier in s_regular_platoon.1.0.0.0.iter() {
                        if let Some(client_entity) = entity_maps.server_to_client.get(soldier) {
                            let _ = soldiers.insert(*client_entity);
                        }
                    }

                    for specialist in s_regular_platoon.1.0.0.1.iter() {
                        if let Some(client_entity) = entity_maps.server_to_client.get(specialist) {
                            let _ = specialists.insert(*client_entity);
                        }
                    }

                    let mut leader = Entity::PLACEHOLDER;
                    if let Some(client_entity) = entity_maps.server_to_client.get(&s_regular_platoon.1.2) {
                        leader = *client_entity;
                    }

                    regular_platoons.insert(s_regular_platoon.0, (RegularSquad((soldiers, specialists)), s_regular_platoon.1.1.clone(), leader));
                }

                for s_shock_platoon in army.shock_platoons.iter() {
                    let mut soldiers: LimitedHashSet<Entity, SHOCK_SQUAD_SIZE> = LimitedHashSet::new();
                    let mut specialists: LimitedHashSet<Entity, SPECIALISTS_PER_SHOCK_SQUAD> = LimitedHashSet::new();

                    for soldier in s_shock_platoon.1.0.0.0.iter() {
                        if let Some(client_entity) = entity_maps.server_to_client.get(soldier) {
                            let _ = soldiers.insert(*client_entity);
                        }
                    }

                    for specialist in s_shock_platoon.1.0.0.1.iter() {
                        if let Some(client_entity) = entity_maps.server_to_client.get(specialist) {
                            let _ = specialists.insert(*client_entity);
                        }
                    }

                    let mut leader = Entity::PLACEHOLDER;
                    if let Some(client_entity) = entity_maps.server_to_client.get(&s_shock_platoon.1.2) {
                        leader = *client_entity;
                    }

                    shock_platoons.insert(s_shock_platoon.0, (ShockSquad((soldiers, specialists)), s_shock_platoon.1.1.clone(), leader));
                }

                for s_armored_platoon in army.armored_platoons.iter() {
                    let mut vehicles: LimitedHashSet<Entity, ARMORED_SQUAD_SIZE> = LimitedHashSet::new();

                    for vehicle in s_armored_platoon.1.0.0.iter() {
                        if let Some(client_entity) = entity_maps.server_to_client.get(vehicle) {
                            let _ = vehicles.insert(*client_entity);
                        }
                    }

                    let mut leader = Entity::PLACEHOLDER;
                    if let Some(client_entity) = entity_maps.server_to_client.get(&s_armored_platoon.1.2) {
                        leader = *client_entity;
                    }

                    armored_platoons.insert(s_armored_platoon.0, (ArmoredSquad(vehicles), s_armored_platoon.1.1.clone(), leader));
                }

                for s_artillery in army.artillery_units.0.iter() {
                    if let Some(server_entity) = s_artillery.1.0.0 {
                        if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                            artillery_units.0.insert(s_artillery.0, (
                                (
                                    Some(*client_entity),
                                    s_artillery.1.0.1.clone(),
                                ),
                                s_artillery.1.1,
                            ));
                        }
                    } else {
                        artillery_units.0.insert(s_artillery.0, s_artillery.1.clone());
                    }
                }

                for s_engineer in army.engineers.iter() {
                    if let Some(server_entity) = s_engineer.1.0.0 {
                        if let Some(client_entity) = entity_maps.server_to_client.get(&server_entity) {
                            engineers.insert(s_engineer.0, (
                                (
                                    Some(*client_entity),
                                    s_engineer.1.0.1.clone(),
                                ),
                                s_engineer.1.1,
                            ));
                        }
                    } else {
                        engineers.insert(s_engineer.0, s_engineer.1.clone());
                    }
                }

                armies.0.insert(1, ArmyObject{
                    regular_squads: regular_platoons,
                    shock_squads: shock_platoons,
                    armored_squads: armored_platoons,
                    artillery_units,
                    engineers,
                });
            },
        }
    }
}

#[derive(Event)]
pub struct AllPlayersPlacedSettlementsEvent;

#[derive(Event)]
pub struct ClientGameInitializedEvent;

#[derive(Event)]
pub struct ClientGameStartedEvent;

pub fn client_game_initialization_system(
    mut event_reader: EventReader<ClientGameInitializedEvent>,
    mut next_state: ResMut<NextState<GameState>>,
){
    for _event in event_reader.read(){
        next_state.set(GameState::MultiplayerAsClient);
    }
}

pub fn client_settlements_placement_completion(
    mut game_stage: ResMut<GameStage>,
    mut event_reader: EventReader<AllPlayersPlacedSettlementsEvent>,
    mut event_writer: (
        EventWriter<AllSettlementsPlaced>,
        EventWriter<DeleteTemporaryObjects>,
    ),
){
    for _event in event_reader.read() {
        game_stage.0 = GameStages::BuildingsSetup;

        event_writer.0.send(AllSettlementsPlaced);
        event_writer.1.send(DeleteTemporaryObjects);
    }
}

pub fn client_game_starting_system(
    mut event_reader: EventReader<ClientGameStartedEvent>,

    mut game_stage: ResMut<GameStage>,
    delete_after_start_q: Query<Entity, With<DeleteAfterStart>>,
    mut army: ResMut<Armies>,
    mut commands: Commands,
){
    for _event in event_reader.read() {
        game_stage.0 = GameStages::GameStarted;

        for to_delete in delete_after_start_q.iter() {
            commands.entity(to_delete).despawn();
        }

        let mut platoon_id: LimitedNumber<1, 3> = LimitedNumber::new();
        let mut company_id: LimitedNumber<1, 3> = LimitedNumber::new();
        let mut batallion_id: LimitedNumber<1, 3> = LimitedNumber::new();
        let mut brigade_id: LimitedNumber<1, 3> = LimitedNumber::new();
        let mut division_id: LimitedNumber<1, 2> = LimitedNumber::new();
        platoon_id.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT {
            if platoon_id.next() {
                if company_id.next() {
                    if batallion_id.next() {
                        if brigade_id.next() {
                            division_id.next();
                        }
                    }
                }
            }

            army.0.get_mut(&1).unwrap().regular_squads.insert((
                division_id.get_value(),
                brigade_id.get_value(),
                batallion_id.get_value(),
                company_id.get_value(),
                platoon_id.get_value(),
            ), (RegularSquad((LimitedHashSet::new(), LimitedHashSet::new())), "atgm".to_string(), Entity::PLACEHOLDER));
        }

        for _i in 0..START_SHOCK_SQUADS_AMOUNT {
            if platoon_id.next() {
                if company_id.next() {
                    if batallion_id.next() {
                        if brigade_id.next() {
                            division_id.next();
                        }
                    }
                }
            }

            army.0.get_mut(&1).unwrap().shock_squads.insert((
                division_id.get_value(),
                brigade_id.get_value(),
                batallion_id.get_value(),
                company_id.get_value(),
                platoon_id.get_value(),
            ), (ShockSquad((LimitedHashSet::new(), LimitedHashSet::new())), "lat".to_string(), Entity::PLACEHOLDER));
        }

        for _i in 0..START_ARMORED_SQUADS_AMOUNT {
            if platoon_id.next() {
                if company_id.next() {
                    if batallion_id.next() {
                        if brigade_id.next() {
                            division_id.next();
                        }
                    }
                }
            }

            army.0.get_mut(&1).unwrap().armored_squads.insert((
                division_id.get_value(),
                brigade_id.get_value(),
                batallion_id.get_value(),
                company_id.get_value(),
                platoon_id.get_value(),
            ), (ArmoredSquad(LimitedHashSet::new()), "tank".to_string(), Entity::PLACEHOLDER));
        }

        for i in 1..START_ARTILLERY_UNITS_COUNT + 1 {
            army.0.get_mut(&1).unwrap().artillery_units.0.insert(i, ((None, "artillery".to_string()), Entity::PLACEHOLDER));
        }

        for i in 1..START_ENGINEERS_COUNT + 1 {
            army.0.get_mut(&1).unwrap().engineers.insert(i, ((None, "engineer".to_string()), Entity::PLACEHOLDER));
        }
    }
}

pub fn client_entity_movement_system(
    mut unspecified_entities_to_move: ResMut<UnspecifiedEntitiesToMove>,
    mut units_to_insert_path: ResMut<UnitsToInsertPath>,
    mut moving_entities_q: Query<&mut Transform>,
    mut units_q: Query<&mut UnitComponent>,
    mut commands: Commands,
){
    if !unspecified_entities_to_move.0.is_empty(){
        for entity in unspecified_entities_to_move.0.iter() {
            if let Ok(mut transform) = moving_entities_q.get_mut(entity.0) {
                if let Ok(mut unit_component) = units_q.get_mut(entity.0) {
                    unit_component.path.clear();
                } else {
                    transform.look_at(entity.1, Vec3::Y);
                }

                transform.translation = entity.1;
            }
        }

        unspecified_entities_to_move.0.clear();
    }

    if !units_to_insert_path.0.is_empty(){
        for unit in units_to_insert_path.0.iter() {
            if let Ok(mut unit_component) = units_q.get_mut(unit.0) {
                unit_component.path = unit.1.clone();
                unit_component.elapsed = 0.;

                commands.entity(unit.0).try_insert(NeedToMove);
            }
        }

        units_to_insert_path.0.clear();
    }
}