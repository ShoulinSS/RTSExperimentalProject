use core::f32;
use std::{hash, process::Command};
use bevy::{ecs::event::{self, event_update_condition}, math::VectorSpace, pbr::{ExtendedMaterial, NotShadowCaster}, prelude::*, utils::hashbrown::{HashMap, HashSet}};
use bevy_mod_raycast::{cursor::CursorRay, prelude::{Raycast, RaycastSettings}};
use bevy_quinnet::{client::QuinnetClient, server::QuinnetServer};
use bevy_rapier3d::{na::ComplexField, plugin::RapierContext, prelude::{CharacterLength, Collider, CollisionGroups, ComputedColliderShape, Group, KinematicCharacterController, LockedAxes, QueryFilter, RigidBody}};
use oxidized_navigation_serializable::{colliders, query::{find_polygon_path, perform_string_pulling_on_path}, Area, NavMesh, NavMeshAffector, NavMeshAreaType, NavMeshSettings};
use rand::Rng;
use serde::{de, Deserialize, Serialize};
use crate::{GameStage, GameStages, PlayerData, WORLD_SIZE, components::{asset_manager::{AnimationComponent, BuildingsAssets, ChangeMaterial, CircleData, CircleHolder, ForbiddenBlueprint, InstancedMaterials, LOD, TeamMaterialExtension, Terrain, UnitAssets}, camera::{self, CameraComponent, SelectionBounds, SelectionBox}, logistics::LogisticUnitComponent, network::EntityMaps, ui_manager::{ActivateBlueprintsDeletionMode, ActivateBuildingsDeletionCancelationMode, ActivateBuildingsDeletionMode, DisplayedModelHolder, OpenBuildingsListEvent, RebuildApartments, SwitchBuildingState}, unit::{self, AttackAnimationTypes, BusyEngineer, DeleteAfterStart, EngineerActions, InfantryTransport, IsUnitSelectionAllowed, NeedToMove, RemainsCount, StoppedMoving, UnitRemains}}};

use super::{asset_manager::{generate_circle_segments, LineData, LineHolder}, logistics::{create_curved_mesh, create_plane_between_points, ResourceZone, RESOURCE_ZONES_COUNT /*RoadComponent, RoadObject*/}, network::{ClientList, ClientMessage, NetworkStatus, NetworkStatuses, ServerMessage}, ui_manager::{Actions, ButtonAction, GameStartedEvent, ProductionStateChanged, UiButtonNodes}, unit::{Armies, ArtilleryUnit, AttackTypes, CompanyTypes, CombatComponent, EngineerComponent, SelectableUnit, SuppliesConsumerComponent, UnitComponent, UnitDeathEvent, UnitNeedsToBeUncovered, UnitTypes, UnitsTileMap, TILE_SIZE}};

#[derive(Event)]
pub struct ProductionButtonPressed {
    pub data: String,
}

#[derive(Event)]
pub struct AllSettlementsPlaced;

#[derive(Event)]
pub struct AllApartmentsPlaced;

#[derive(Event)]
pub struct AllRoadsGenerated;

#[derive(Resource)]
pub struct ProductionState {
    pub is_allowed: HashMap<i32, bool>,
}

#[derive(Resource, Clone)]
pub struct ProductionQueue (pub HashMap<i32, ProductionQueueObject>);

#[derive(Clone)]
pub struct ProductionQueueObject{
    pub regular_infantry_queue: HashMap<(i32, i32, i32, i32, i32, i32, i32), (String, Entity)>,
    pub shock_infantry_queue: HashMap<(i32, i32, i32, i32, i32, i32, i32), (String, Entity)>,
    pub vehicles_queue: HashMap<(i32, i32, i32, i32, i32, i32, i32), (String, Entity)>,
    pub artillery_queue: HashMap<(i32, i32, i32, i32, i32, i32, i32), (String, Entity)>,
    pub engineers_queue: HashMap<(i32, i32, i32, i32, i32, i32, i32), (String, Entity)>,
}

#[derive(Resource)]
pub struct SelectedBuildings {
    pub buildings: Vec<Entity>,
}

#[derive(Clone)]
pub struct InfantryBarracksBundle {
    pub model: PbrBundle,
    pub collider: Collider,
    pub building_component: UnitProductionBuildingComponent,
    pub combat_component: CombatComponent,
    pub selectable: SelectableBuilding,
    pub producer: InfantryProducer,
    pub human_resource_storage: HumanResourceStorageComponent,
    pub materials_storage: MaterialsStorageComponent,
}

#[derive(Clone)]
pub struct VehicleFactoryBundle {
    pub model: PbrBundle,
    pub collider: Collider,
    pub building_component: UnitProductionBuildingComponent,
    pub combat_component: CombatComponent,
    pub selectable: SelectableBuilding,
    pub producer: VehiclesProducer,
    pub human_resource_storage: HumanResourceStorageComponent,
    pub materials_storage: MaterialsStorageComponent,
}

#[derive(Clone)]
pub struct LogisticHubBundle {
    pub model: PbrBundle,
    pub collider: Collider,
    pub building_component: SuppliesProductionComponent,
    pub storage: MaterialsStorageComponent,
    pub combat_component: CombatComponent,
}

#[derive(Clone)]
pub struct ResourceMinerBundle {
    pub model: PbrBundle,
    pub collider: Collider,
    pub building_component: MaterialsProductionComponent,
    pub combat_component: CombatComponent,
}

#[derive(Clone)]
pub struct PillboxBundle {
    pub model: PbrBundle,
    pub collider: Collider,
    pub building_component: CoverComponent,
    pub combat_component: CombatComponent,
}

#[derive(Clone)]
pub enum BuildingsBundles {
    InfantryBarracks(InfantryBarracksBundle),
    VehicleFactory(VehicleFactoryBundle),
    LogisticHub(LogisticHubBundle),
    ResourceMiner(ResourceMinerBundle),
    Pillbox(PillboxBundle),
    None,
}

#[derive(Resource)]
pub struct BuildingsList(pub Vec<(String, BuildingsBundles, Collider, f32, i32, f32, i32)>);

#[derive(Resource)]
pub struct UnactivatedBlueprints(pub HashMap<i32, HashMap<Entity, (Vec3, Entity, f32)>>);

pub fn add_selected_buildings(
    buildings: Vec<Entity>,
    selected_buildings: &mut ResMut<SelectedBuildings>,
    commands: &mut Commands,
    buildings_q: &Query<Entity, With<SelectableBuilding>>,
){
    for building in buildings {
        if buildings_q.get(building).is_ok() && !selected_buildings.buildings.contains(&building) {
            selected_buildings.buildings.push(building);
            commands.entity(building).insert(SelectedBuilding);
        }
    }
}

pub fn clear_selected_buildings(
    selected_buildings: &mut ResMut<SelectedBuildings>,
    commands: &mut Commands,
    buildings_q: &Query<Entity, With<SelectableBuilding>>,
){
    for building in selected_buildings.buildings.clone(){
        if buildings_q.get(building).is_ok() {
            commands.entity(building).remove::<SelectedBuilding>();
        }
    }
    selected_buildings.buildings.clear();
}

#[derive(Component, Clone)]
pub struct SelectableBuilding;

#[derive(Component)]
pub struct SelectedBuilding;

#[derive(Clone)]
pub struct ProductionData {
    pub time_to_produce: u128,
    pub resource_cost: i32,
    pub human_resource_cost: i32,
}

#[derive(Component)]
pub struct NeedToProduce;

#[derive(Component)]
pub struct BuildingBlueprint {
    pub team: i32,
    pub building_bundle: BuildingsBundles,
    pub build_power_remaining: i32,
    pub name: String,
    pub build_distance: f32,
    pub resource_cost: i32,
}

#[derive(Component)]
pub struct BuildingConstructionSite {
    pub team: i32,
    pub building_bundle: BuildingsBundles,
    pub build_power_total: i32,
    pub build_power_remaining: i32,
    pub name: String,
    pub build_distance: f32,
    pub current_builder: Entity,
    pub resource_cost: i32,
}

#[derive(Clone)]
pub struct SoldierBundle {
    pub scene: Handle<Scene>,
    pub lod: PbrBundle,
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub selectable: SelectableUnit,
    pub controller: KinematicCharacterController,
    pub animation_component: AnimationComponent,
    pub material_change_marker: ChangeMaterial,
}

#[derive(Clone)]
pub struct AssaultBundle {
    pub scene: Handle<Scene>,
    pub lod: PbrBundle,
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub selectable: SelectableUnit,
    pub controller: KinematicCharacterController,
    pub animation_component: AnimationComponent,
    pub material_change_marker: ChangeMaterial,
}

#[derive(Clone)]
pub struct TankBundle {
    pub model_hull: PbrBundle,
    pub model_turret: PbrBundle,
    pub lod: (PbrBundle, PbrBundle),
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub selectable: SelectableUnit,
    pub controller: KinematicCharacterController,
}

#[derive(Clone)]
pub struct IFVBundle {
    pub model_hull: PbrBundle,
    pub model_turret: PbrBundle,
    pub lod: (PbrBundle, PbrBundle),
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub transport: InfantryTransport,
    pub selectable: SelectableUnit,
    pub controller: KinematicCharacterController,
}

#[derive(Clone)]
pub struct ArtilleryBundle {
    pub model: PbrBundle,
    pub lod: PbrBundle,
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub artillery_component: ArtilleryUnit,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub selectable: SelectableUnit,
    pub controller: KinematicCharacterController,
}

#[derive(Clone)]
pub struct EngineerBundle {
    pub model: PbrBundle,
    pub lod: PbrBundle,
    pub unit_component: UnitComponent,
    pub combat_component: CombatComponent,
    pub engineer_component: EngineerComponent,
    pub supplies_consumer: SuppliesConsumerComponent,
    pub controller: KinematicCharacterController,
}

#[derive(Clone)]
pub enum UnitBundles {
    Soldier(SoldierBundle),
    Shock(AssaultBundle),
    Tank(TankBundle),
    IFV(IFVBundle),
    Artillery(ArtilleryBundle),
    Engineer(EngineerBundle),
}

#[derive(Component, Clone)]
pub struct UnitProductionBuildingComponent {
    pub available_to_build: HashMap<String, (UnitBundles, ProductionData)>,
    pub build_order: Vec<(i32, UnitBundles, ProductionData, CompanyTypes, (i32, i32, i32, i32, i32, i32, i32), String)>,
    pub spawn_point: Vec3,
    pub elapsed_time: u128,
}

#[derive(Component, Clone)]
pub struct InfantryProducer;

#[derive(Component, Clone)]
pub struct VehiclesProducer;

#[derive(Component)]
pub struct SuppliesStorageComponent {
    pub supplies_storage_capacity: i32,
    pub available_supplies: i32,
}

#[derive(Component, Clone)]
pub struct MaterialsStorageComponent {
    pub materials_storage_capacity: i32,
    pub available_resources: i32,
    pub replenishment_amount: i32,
    pub replenishment_cooldown: u128,
    pub replenishment_time_elapsed: u128,
    pub replenishment_local_point: Vec3,
    pub replenishment_range: f32,
}

#[derive(Component, Clone)]
pub struct HumanResourceStorageComponent {
    pub human_resource_storage_capacity: i32,
    pub available_human_resources: i32,
    pub replenishment_amount: i32,
    pub replenishment_cooldown: u128,
    pub replenishment_time_elapsed: u128,
    pub replenishment_local_point: Vec3,
    pub replenishment_range: f32,
}

#[derive(Component, Clone)]
pub struct SuppliesProductionComponent {
    pub supplies_storage_capacity: i32,
    pub available_supplies: i32,
    pub supplies_production: (i32, ProductionData),
    pub production_local_point: Vec3,
    pub elapsed_production_time: u128,
    pub supply_cooldown: u128,
    pub elapsed_cooldown_time: u128,
}

#[derive(Component, Clone)]
pub struct MaterialsProductionComponent {
    pub materials_storage_capacity: i32,
    pub available_materials: i32,
    pub materials_production_rate: i32,
    pub materials_production_speed: u128,
    pub production_local_point: Vec3,
    pub elapsed_time: u128,
}

#[derive(Component, Clone)]
pub struct HumanResourceProductionComponent {
    pub human_resource_storage_capacity: i32,
    pub available_human_resources: i32,
    pub human_resource_production_rate: i32,
    pub elapsed_time: u128,
}

#[derive(Component, Clone)]
pub struct CoverComponent {
    pub cover_efficiency: f32,
    pub points: Vec<Vec3>,
    pub units_inside: HashSet<Entity>,
}

#[derive(Component)]
pub struct ApartmentHouse;

#[derive(Resource, Clone)]
pub struct SettlementsLeft(pub Vec<(
    SettlementObject,
    PbrBundle,          //ForbiddenZone
)>);

#[derive(Component)]
pub struct SettlementComponent(pub SettlementObject);

#[derive(Clone, Serialize, Deserialize)]
pub struct SettlementObject {
    pub team: i32,
    pub settlement_size: f32,
    pub buffer_zone_size: f32,
    pub max_road_connection_distance: f32,
    pub connected_roads: Vec<Entity>,
    pub connected_settlements: Vec<Entity>,

    pub active_apartments: Vec<(Entity, Vec3, f32)>,
    pub human_resource_storage_capacity: i32,
    pub available_human_resources: i32,
    pub human_resource_production_rate: i32,
    pub human_resource_production_speed: u128,
    pub production_local_point: Vec3,
    pub elapsed_time: u128,

    pub time_to_capture: u128,
    pub elapsed_capture_time: u128,
}

#[derive(Component)]
pub struct TemporaryObject;

#[derive(Event)]
pub struct DeleteTemporaryObjects;

pub const CITIES_COUNT: i32 = 3;
pub const VILLAGES_COUNT: i32 = 6;

pub const ALLOWED_DISTANCE_FROM_BORDERS: f32 = 150.;

#[derive(Resource)]
pub struct ProducableUnits {
    pub barrack_producables: HashMap<String, (UnitBundles, ProductionData)>,
    pub factory_producables: HashMap<String, (UnitBundles, ProductionData)>,
}

pub fn unit_production_system (
    mut infantry_producers_q:
    Query<(Entity, &mut UnitProductionBuildingComponent, &Transform, &CombatComponent, &mut MaterialsStorageComponent, &mut HumanResourceStorageComponent, &SwitchableBuilding),
    (With<InfantryProducer>, Without<VehiclesProducer>)>,
    mut vehicles_producers_q:
    Query<(Entity, &mut UnitProductionBuildingComponent, &Transform, &CombatComponent, &mut MaterialsStorageComponent, &mut HumanResourceStorageComponent, &SwitchableBuilding),
    (With<VehiclesProducer>, Without<InfantryProducer>)>,
    mut production_queue: ResMut<ProductionQueue>,
    production_states: Res<ProductionState>,
    mut commands: Commands,
    mut army: ResMut<Armies>,
    game_stage: Res<GameStage>,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    materials: Res<Assets<StandardMaterial>>,
    mut instanced_materials: ResMut<InstancedMaterials>,
    mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    mut tile_map: ResMut<UnitsTileMap>,
){
    for (team, production_state) in production_states.is_allowed.iter() {
        if *production_state && matches!(game_stage.0, GameStages::GameStarted) &&
        (
            !production_queue.0.get(team).unwrap().regular_infantry_queue.is_empty() ||
            !production_queue.0.get(team).unwrap().shock_infantry_queue.is_empty() ||
            !production_queue.0.get(team).unwrap().vehicles_queue.is_empty() ||
            !production_queue.0.get(team).unwrap().artillery_queue.is_empty() ||
            !production_queue.0.get(team).unwrap().engineers_queue.is_empty()
        ){
            let mut units_queue_iter = production_queue.0.get_mut(team).unwrap().regular_infantry_queue.iter_mut();
            let mut current_infantry_type: CompanyTypes = CompanyTypes::Regular;
            let mut unit_to_produce_extracted;
            let mut unit_to_produce_data_extracted;
        
            let mut regular_infantry_queue_to_delete: Vec<(i32, i32, i32, i32, i32, i32, i32)> = Vec::new();
            let mut shock_infantry_queue_to_delete: Vec<(i32, i32, i32, i32, i32, i32, i32)> = Vec::new();
            let mut vehicles_queue_to_delete: Vec<(i32, i32, i32, i32, i32, i32, i32)> = Vec::new();
            let mut artillery_queue_to_delete: Vec<(i32, i32, i32, i32, i32, i32, i32)> = Vec::new();
            let mut engineers_queue_to_delete: Vec<(i32, i32, i32, i32, i32, i32, i32)> = Vec::new();

            let color;
            let simplified_material;
            if *team == 1 {
                color = Vec4::new(0., 0., 1., 1.);
                simplified_material = instanced_materials.blue_solid.clone();
            } else {
                color = Vec4::new(1., 0., 0., 1.);
                simplified_material = instanced_materials.red_solid.clone();
            }
        
            for mut infantry_producer in infantry_producers_q.iter_mut() {
                if infantry_producer.3.team != *team || !infantry_producer.6.0 {
                    continue;
                }
                
                if infantry_producer.1.build_order.len() > 0 {
                    infantry_producer.1.elapsed_time += time.delta().as_millis();
        
                    if infantry_producer.1.elapsed_time >= infantry_producer.1.build_order[0].2.time_to_produce {
                        if
                        infantry_producer.4.available_resources < infantry_producer.1.build_order[0].2.resource_cost ||
                        infantry_producer.5.available_human_resources < infantry_producer.1.build_order[0].2.human_resource_cost {
                            continue;
                        } else {
                            infantry_producer.4.available_resources -= infantry_producer.1.build_order[0].2.resource_cost;
                            infantry_producer.5.available_human_resources -= infantry_producer.1.build_order[0].2.human_resource_cost;

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::MaterialsDelivered {
                                        server_entity: infantry_producer.0,
                                        amount: -infantry_producer.1.build_order[0].2.resource_cost,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }

                                channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::HumanResourcesDelivered {
                                        server_entity: infantry_producer.0,
                                        amount: -infantry_producer.1.build_order[0].2.human_resource_cost,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                        
                        infantry_producer.1.elapsed_time = 0;
        
                        let mut new_unit= Entity::PLACEHOLDER;

                        let new_unit_position = infantry_producer.2.transform_point(infantry_producer.1.spawn_point);

                        let tile = ((new_unit_position.x / TILE_SIZE) as i32, (new_unit_position.z / TILE_SIZE) as i32);
                        let unit_type;
        
                        match &infantry_producer.1.build_order[0].1 {
                            UnitBundles::Soldier(b) => {
                                unit_type = b.combat_component.unit_type.clone();
                                
                                new_unit = commands.spawn((
                                    SceneBundle{
                                        scene: b.scene.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                infantry_producer.1.build_order[0].3,
                                                infantry_producer.1.build_order[0].4,
                                                infantry_producer.1.build_order[0].5.clone(),
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
                            UnitBundles::Shock(b) => {
                                unit_type = b.combat_component.unit_type.clone();

                                new_unit = commands.spawn((
                                    SceneBundle{
                                        scene: b.scene.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                infantry_producer.1.build_order[0].3,
                                                infantry_producer.1.build_order[0].4,
                                                infantry_producer.1.build_order[0].5.clone(),
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team)) {
                                    material_turret = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_turret.material.id()) {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team), material_turret.clone());
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team)) {
                                    material_hull = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_hull.material.id()) {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team), material_hull.clone());
                                }

                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model_hull.mesh.clone(),
                                        material: material_hull.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                infantry_producer.1.build_order[0].3,
                                                infantry_producer.1.build_order[0].4,
                                                infantry_producer.1.build_order[0].5.clone(),
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team)) {
                                    material_turret = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_turret.material.id()) {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team), material_turret.clone());
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team)) {
                                    material_hull = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_hull.material.id()) {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team), material_hull.clone());
                                }

                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model_hull.mesh.clone(),
                                        material: material_hull.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                infantry_producer.1.build_order[0].3,
                                                infantry_producer.1.build_order[0].4,
                                                infantry_producer.1.build_order[0].5.clone(),
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
        
                        match infantry_producer.1.build_order[0].3 {
                            CompanyTypes::Regular => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().regular_squads.get_mut(&(
                                    infantry_producer.1.build_order[0].4.0,
                                    infantry_producer.1.build_order[0].4.1,
                                    infantry_producer.1.build_order[0].4.2,
                                    infantry_producer.1.build_order[0].4.3,
                                    infantry_producer.1.build_order[0].4.4,
                                )) {
                                    if infantry_producer.1.build_order[0].4.5 == 0 {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.0.insert(new_unit);
                                        }
                                    } else {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.1.insert(new_unit);
                                        }
                                    }
                                }
        
                                regular_infantry_queue_to_delete.push(infantry_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Shock => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().shock_squads.get_mut(&(
                                    infantry_producer.1.build_order[0].4.0,
                                    infantry_producer.1.build_order[0].4.1,
                                    infantry_producer.1.build_order[0].4.2,
                                    infantry_producer.1.build_order[0].4.3,
                                    infantry_producer.1.build_order[0].4.4,
                                )) {
                                    if infantry_producer.1.build_order[0].4.5 == 0 {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.0.insert(new_unit);
                                        }
                                    } else {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.1.insert(new_unit);
                                        }
                                    }
                                }
        
                                shock_infantry_queue_to_delete.push(infantry_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Armored => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().armored_squads.get_mut(&(
                                    infantry_producer.1.build_order[0].4.0,
                                    infantry_producer.1.build_order[0].4.1,
                                    infantry_producer.1.build_order[0].4.2,
                                    infantry_producer.1.build_order[0].4.3,
                                    infantry_producer.1.build_order[0].4.4,
                                )) {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.insert(new_unit);
                                    }
                                }
        
                                vehicles_queue_to_delete.push(infantry_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Artillery => {
                                if let Some(artillery_unit) =
                                army.0.get_mut(team).unwrap().artillery_units.0.get_mut(&infantry_producer.1.build_order[0].4.6){
                                    if new_unit != Entity::PLACEHOLDER {
                                        artillery_unit.0.0 = Some(new_unit);
                                    }
                                }
    
                                artillery_queue_to_delete.push(infantry_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Engineer => {
                                if let Some(engineer) =
                                army.0.get_mut(team).unwrap().engineers.get_mut(&infantry_producer.1.build_order[0].4.6){
                                    if new_unit != Entity::PLACEHOLDER {
                                        engineer.0.0 = Some(new_unit);
                                    }
                                }
    
                                engineers_queue_to_delete.push(infantry_producer.1.build_order[0].4);
                            },
                            CompanyTypes::None => {},
                        }

                        tile_map.tiles.entry(*team).or_insert_with(HashMap::new).entry(tile)
                        .or_insert_with(HashMap::new).insert(new_unit, (new_unit_position, unit_type));
    
                        if matches!(network_status.0, NetworkStatuses::Host) {
                            let mut channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                    unit_data: (
                                        *team,
                                        (
                                            infantry_producer.1.build_order[0].3,
                                            infantry_producer.1.build_order[0].4,
                                            infantry_producer.1.build_order[0].5.clone(),
                                        ),
                                    ),
                                    position: new_unit_position,
                                    server_entity: new_unit,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        }
    
                        infantry_producer.1.build_order.remove(0);
                    }
                } else {
                    unit_to_produce_extracted = None;
                    loop {
                        if let Some(unit_to_produce) = units_queue_iter.next() {
                            if commands.get_entity(unit_to_produce.1.1).is_none() {
                                unit_to_produce_extracted = Some(unit_to_produce);
                                break;
                            }
                        }
                        else {
                            break;
                        }
                    }
        
                    if let Some(unit_to_produce) = unit_to_produce_extracted {
                        if let Some(unit_to_produce_data) = infantry_producer.1.available_to_build.get(&unit_to_produce.1.0.clone()) {
                            unit_to_produce_data_extracted = unit_to_produce_data.clone();
                        } else {
                            continue;
                        }
            
                        infantry_producer.1.build_order.push((
                            *team,
                            unit_to_produce_data_extracted.0.clone(),
                            unit_to_produce_data_extracted.1.clone(),
                            current_infantry_type,
                            *unit_to_produce.0,
                            unit_to_produce.1.0.clone(),
                        ));
        
                        unit_to_produce.1.1 = infantry_producer.0;
                    } else {
                        units_queue_iter = production_queue.0.get_mut(team).unwrap().shock_infantry_queue.iter_mut();
                        current_infantry_type = CompanyTypes::Shock;
            
                        unit_to_produce_extracted = None;
                        loop {
                            if let Some(unit_to_produce) = units_queue_iter.next() {
                                if commands.get_entity(unit_to_produce.1.1).is_none() {
                                    unit_to_produce_extracted = Some(unit_to_produce);
                                    break;
                                }
                            }
                            else {
                                break;
                            }
                        }
            
                        if let Some(unit_to_produce) = unit_to_produce_extracted {
                            if let Some(unit_to_produce_data) = infantry_producer.1.available_to_build.get(&unit_to_produce.1.0.clone()) {
                                unit_to_produce_data_extracted = unit_to_produce_data.clone();
                            } else {
                                continue;
                            }
                
                            infantry_producer.1.build_order.push((
                                *team,
                                unit_to_produce_data_extracted.0.clone(),
                                unit_to_produce_data_extracted.1.clone(),
                                current_infantry_type,
                                *unit_to_produce.0,
                                unit_to_produce.1.0.clone(),
                            ));
            
                            unit_to_produce.1.1 = infantry_producer.0;
                        } else {
                            break;
                        }
                    }
                }
            }
    
            units_queue_iter = production_queue.0.get_mut(team).unwrap().vehicles_queue.iter_mut();
    
            for mut vehicles_producer in vehicles_producers_q.iter_mut() {
                if vehicles_producer.3.team != *team || !vehicles_producer.6.0 {
                    continue;
                }

                if vehicles_producer.1.build_order.len() > 0 {
                    vehicles_producer.1.elapsed_time += time.delta().as_millis();
        
                    if vehicles_producer.1.elapsed_time >= vehicles_producer.1.build_order[0].2.time_to_produce {
                        if
                        vehicles_producer.4.available_resources < vehicles_producer.1.build_order[0].2.resource_cost ||
                        vehicles_producer.5.available_human_resources < vehicles_producer.1.build_order[0].2.human_resource_cost {
                            continue;
                        } else {
                            vehicles_producer.4.available_resources -= vehicles_producer.1.build_order[0].2.resource_cost;
                            vehicles_producer.5.available_human_resources -= vehicles_producer.1.build_order[0].2.human_resource_cost;

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::MaterialsDelivered {
                                        server_entity: vehicles_producer.0,
                                        amount: -vehicles_producer.1.build_order[0].2.resource_cost,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }

                                channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::HumanResourcesDelivered {
                                        server_entity: vehicles_producer.0,
                                        amount: -vehicles_producer.1.build_order[0].2.human_resource_cost,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }

                        vehicles_producer.1.elapsed_time = 0;
        
                        let mut new_unit = Entity::PLACEHOLDER;

                        let new_unit_position = vehicles_producer.2.transform_point(vehicles_producer.1.spawn_point);

                        let tile = ((new_unit_position.x / TILE_SIZE) as i32, (new_unit_position.z / TILE_SIZE) as i32);
                        let unit_type;
        
                        match &vehicles_producer.1.build_order[0].1 {
                            UnitBundles::Soldier(b) => {
                                unit_type = b.combat_component.unit_type.clone();

                                new_unit = commands.spawn((
                                    SceneBundle{
                                        scene: b.scene.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
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
                            UnitBundles::Shock(b) => {
                                unit_type = b.combat_component.unit_type.clone();
                                
                                new_unit = commands.spawn((
                                    SceneBundle{
                                        scene: b.scene.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team)) {
                                    material_turret = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_turret.material.id()) {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team), material_turret.clone());
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team)) {
                                    material_hull = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_hull.material.id()) {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team), material_hull.clone());
                                }

                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model_hull.mesh.clone(),
                                        material: material_hull.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team)) {
                                    material_turret = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_turret.material.id()) {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_turret = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team), material_turret.clone());
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team)) {
                                    material_hull = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model_hull.material.id()) {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material_hull = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team), material_hull.clone());
                                }
                                
                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model_hull.mesh.clone(),
                                        material: material_hull.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
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
                            UnitBundles::Artillery(b) => {
                                unit_type = b.combat_component.unit_type.clone();

                                let material;

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model.mesh.id(), *team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model.material.id()) {
                                        material = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model.mesh.id(), *team), material.clone());
                                }

                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model.mesh.clone(),
                                        material: material.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
                                            ),
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

                                if let Some(mat) = instanced_materials.team_materials.get(&(b.model.mesh.id(), *team)) {
                                    material = mat.clone();
                                } else {
                                    if let Some(original) = materials.get(b.model.material.id()) {
                                        material = extended_materials.add(ExtendedMaterial {
                                            base: original.clone(),
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    } else {
                                        material = extended_materials.add(ExtendedMaterial {
                                            base: StandardMaterial{
                                                ..default()
                                            },
                                            extension: TeamMaterialExtension {
                                                team_color: color,
                                            },
                                        });
                                    }

                                    instanced_materials.team_materials.insert((b.model.mesh.id(), *team), material.clone());
                                }
                                
                                new_unit = commands.spawn((
                                    MaterialMeshBundle{
                                        mesh: b.model.mesh.clone(),
                                        material: material.clone(),
                                        transform: Transform::from_translation(new_unit_position),
                                        ..default()
                                    },
                                    b.unit_component.clone(),
                                    CombatComponent {
                                        team: *team,
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
                                                vehicles_producer.1.build_order[0].3,
                                                vehicles_producer.1.build_order[0].4,
                                                vehicles_producer.1.build_order[0].5.clone(),
                                            ),
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
        
                        match vehicles_producer.1.build_order[0].3 {
                            CompanyTypes::Regular => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().regular_squads.get_mut(&(
                                    vehicles_producer.1.build_order[0].4.0,
                                    vehicles_producer.1.build_order[0].4.1,
                                    vehicles_producer.1.build_order[0].4.2,
                                    vehicles_producer.1.build_order[0].4.3,
                                    vehicles_producer.1.build_order[0].4.4,
                                )) {
                                    if vehicles_producer.1.build_order[0].4.5 == 0 {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.0.insert(new_unit);
                                        }
                                    } else {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.1.insert(new_unit);
                                        }
                                    }
                                }
        
                                regular_infantry_queue_to_delete.push(vehicles_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Shock => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().shock_squads.get_mut(&(
                                    vehicles_producer.1.build_order[0].4.0,
                                    vehicles_producer.1.build_order[0].4.1,
                                    vehicles_producer.1.build_order[0].4.2,
                                    vehicles_producer.1.build_order[0].4.3,
                                    vehicles_producer.1.build_order[0].4.4,
                                )) {
                                    if vehicles_producer.1.build_order[0].4.5 == 0 {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.0.insert(new_unit);
                                        }
                                    } else {
                                        if new_unit != Entity::PLACEHOLDER {
                                            let _ = platoon.0.0.1.insert(new_unit);
                                        }
                                    }
                                }
        
                                shock_infantry_queue_to_delete.push(vehicles_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Armored => {
                                if let Some (platoon) = army.0.get_mut(team).unwrap().armored_squads.get_mut(&(
                                    vehicles_producer.1.build_order[0].4.0,
                                    vehicles_producer.1.build_order[0].4.1,
                                    vehicles_producer.1.build_order[0].4.2,
                                    vehicles_producer.1.build_order[0].4.3,
                                    vehicles_producer.1.build_order[0].4.4,
                                )) {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.insert(new_unit);
                                    }
                                }
        
                                vehicles_queue_to_delete.push(vehicles_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Artillery => {
                                if let Some(artillery_unit) =
                                army.0.get_mut(team).unwrap().artillery_units.0.get_mut(&vehicles_producer.1.build_order[0].4.6){
                                    if new_unit != Entity::PLACEHOLDER {
                                        artillery_unit.0.0 = Some(new_unit);
                                    }
                                }
    
                                artillery_queue_to_delete.push(vehicles_producer.1.build_order[0].4);
                            },
                            CompanyTypes::Engineer => {
                                if let Some(engineer) =
                                army.0.get_mut(team).unwrap().engineers.get_mut(&vehicles_producer.1.build_order[0].4.6){
                                    if new_unit != Entity::PLACEHOLDER {
                                        engineer.0.0 = Some(new_unit);
                                    }
                                }
    
                                engineers_queue_to_delete.push(vehicles_producer.1.build_order[0].4);
                            },
                            CompanyTypes::None => {},
                        }

                        tile_map.tiles.entry(*team).or_insert_with(HashMap::new).entry(tile)
                        .or_insert_with(HashMap::new).insert(new_unit, (new_unit_position, unit_type));
    
                        if matches!(network_status.0, NetworkStatuses::Host) {
                            let mut channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                    unit_data: (
                                        *team,
                                        (
                                            vehicles_producer.1.build_order[0].3,
                                            vehicles_producer.1.build_order[0].4,
                                            vehicles_producer.1.build_order[0].5.clone(),
                                        ),
                                    ),
                                    position: new_unit_position,
                                    server_entity: new_unit,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        }
    
                        vehicles_producer.1.build_order.remove(0);
                    }
                } else {
                    unit_to_produce_extracted = None;
    
                    loop {
                        if let Some(unit_to_produce) = units_queue_iter.next() {
                            if commands.get_entity(unit_to_produce.1.1).is_none() {
                                unit_to_produce_extracted = Some(unit_to_produce);
                                break;
                            }
                        }
                        else {
                            break;
                        }
                    }
        
                    if let Some(unit_to_produce) = unit_to_produce_extracted {
                        if let Some(unit_to_produce_data) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0.clone()) {
                            unit_to_produce_data_extracted = unit_to_produce_data.clone();
                        } else {
                            continue;
                        }
            
                        vehicles_producer.1.build_order.push((
                            *team,
                            unit_to_produce_data_extracted.0.clone(),
                            unit_to_produce_data_extracted.1.clone(),
                            CompanyTypes::Armored,
                            *unit_to_produce.0,
                            unit_to_produce.1.0.clone(),
                        ));
        
                        unit_to_produce.1.1 = vehicles_producer.0;
                    } else {
                        unit_to_produce_extracted = None;
                        units_queue_iter = production_queue.0.get_mut(team).unwrap().artillery_queue.iter_mut();
    
                        loop {
                            if let Some(unit_to_produce) = units_queue_iter.next() {
                                if commands.get_entity(unit_to_produce.1.1).is_none() {
                                    unit_to_produce_extracted = Some(unit_to_produce);
                                    break;
                                }
                            }
                            else {
                                break;
                            }
                        }
            
                        if let Some(unit_to_produce) = unit_to_produce_extracted {
                            if let Some(unit_to_produce_data) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0.clone()) {
                                unit_to_produce_data_extracted = unit_to_produce_data.clone();
                            } else {
                                continue;
                            }
                
                            vehicles_producer.1.build_order.push((
                                *team,
                                unit_to_produce_data_extracted.0.clone(),
                                unit_to_produce_data_extracted.1.clone(),
                                CompanyTypes::Artillery,
                                *unit_to_produce.0,
                                unit_to_produce.1.0.clone(),
                            ));
            
                            unit_to_produce.1.1 = vehicles_producer.0;
                        } else {
                            unit_to_produce_extracted = None;
                            units_queue_iter = production_queue.0.get_mut(team).unwrap().engineers_queue.iter_mut();
        
                            loop {
                                if let Some(unit_to_produce) = units_queue_iter.next() {
                                    if commands.get_entity(unit_to_produce.1.1).is_none() {
                                        unit_to_produce_extracted = Some(unit_to_produce);
                                        break;
                                    }
                                }
                                else {
                                    break;
                                }
                            }
                
                            if let Some(unit_to_produce) = unit_to_produce_extracted {
                                if let Some(unit_to_produce_data) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0.clone()) {
                                    unit_to_produce_data_extracted = unit_to_produce_data.clone();
                                } else {
                                    continue;
                                }
                    
                                vehicles_producer.1.build_order.push((
                                    *team,
                                    unit_to_produce_data_extracted.0.clone(),
                                    unit_to_produce_data_extracted.1.clone(),
                                    CompanyTypes::Engineer,
                                    *unit_to_produce.0,
                                    unit_to_produce.1.0.clone(),
                                ));
                
                                unit_to_produce.1.1 = vehicles_producer.0;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        
            for regular_queue_position in regular_infantry_queue_to_delete.iter() {
                production_queue.0.get_mut(team).unwrap().regular_infantry_queue.remove(regular_queue_position);
            }
        
            for shock_queue_position in shock_infantry_queue_to_delete.iter() {
                production_queue.0.get_mut(team).unwrap().shock_infantry_queue.remove(shock_queue_position);
            }
        
            for vehicle_queue_position in vehicles_queue_to_delete.iter() {
                production_queue.0.get_mut(team).unwrap().vehicles_queue.remove(vehicle_queue_position);
            }
    
            for artillery_queue_position in artillery_queue_to_delete.iter() {
                production_queue.0.get_mut(team).unwrap().artillery_queue.remove(artillery_queue_position);
            }
    
            for engineers_queue_position in engineers_queue_to_delete.iter() {
                production_queue.0.get_mut(team).unwrap().engineers_queue.remove(engineers_queue_position);
            }
        }
    }
}

pub fn unit_replenishment_system(
    mut production_queue: ResMut<ProductionQueue>,
    mut event_reader: EventReader<UnitDeathEvent>,
    mut army: ResMut<Armies>,
    mut tile_map: ResMut<UnitsTileMap>,
    unit_assets: Res<UnitAssets>,
    materials: Res<Assets<StandardMaterial>>,
    mut instanced_materials: ResMut<InstancedMaterials>,
    mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    mut remains_count: ResMut<RemainsCount>,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<UnitNeedsToBeUncovered>,
    ),
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    if !event_reader.is_empty() {
        let mut deleted_entities: HashSet<Entity> = HashSet::new();

        for event in event_reader.read() {
            if deleted_entities.get(&event.dead_unit_data.3).is_none() && commands.get_entity(event.dead_unit_data.3).is_some() {
                deleted_entities.insert(event.dead_unit_data.3);

                if event.dead_unit_data.5 {
                    let mut mesh: Handle<Mesh> = Handle::default();
                    let mut material: Handle<StandardMaterial> = Handle::default();
                    let mut remains_type = "unspecified";

                    match event.dead_unit_data.1.1.2.as_str() {
                        "regular_soldier" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "atgm" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "shock_soldier" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "lat" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "sniperr" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "snipers" => {
                            mesh = unit_assets.corpse.0.clone();
                            material = unit_assets.corpse.1.clone();
                            remains_type = "infantry";
                        }
                        "tank" => {
                            mesh = unit_assets.tank.0.clone();
                            material = instanced_materials.wreck_material.clone();
                            remains_type = "vehicle";
                        }
                        "ifv" => {
                            mesh = unit_assets.ifv.0.clone();
                            material = instanced_materials.wreck_material.clone();
                            remains_type = "vehicle";
                        }
                        "artillery" => {
                            mesh = unit_assets.artillery.0.clone();
                            material = instanced_materials.wreck_material.clone();
                            remains_type = "vehicle";
                        }
                        "engineer" => {
                            mesh = unit_assets.engineer.0.clone();
                            material = instanced_materials.wreck_material.clone();
                            remains_type = "vehicle";
                        }
                        _ => {}
                    }

                    remains_count.0 += 1;

                    match remains_type {
                        "infantry" => {
                            let color;
                            let simplified_material;
                            if event.dead_unit_data.0 == 1 {
                                color = Vec4::new(0., 0., 1., 1.);
                                simplified_material = instanced_materials.blue_solid.clone();
                            } else {
                                color = Vec4::new(1., 0., 0., 1.);
                                simplified_material = instanced_materials.red_solid.clone();
                            }

                            let team_material;

                            if let Some(mat) = instanced_materials.team_materials.get(&(mesh.id(), event.dead_unit_data.0)) {
                                team_material = mat.clone();
                            } else {
                                if let Some(original) = materials.get(material.id()) {
                                    team_material = extended_materials.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    team_material = extended_materials.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                instanced_materials.team_materials.insert((mesh.id(), event.dead_unit_data.0), team_material.clone());
                            }

                            commands.spawn(MaterialMeshBundle{
                                mesh: mesh.clone(),
                                material: team_material.clone(),
                                transform: event.dead_unit_data.4,
                                ..default()
                            })
                            .insert(UnitRemains{
                                number: remains_count.0,
                            }).insert(LOD{
                                detailed: (mesh, Some(team_material), None),
                                simplified: (unit_assets.corpse_simplified_mesh.clone(), simplified_material),
                            });
                        }
                        "vehicle" => {
                            commands.spawn(MaterialMeshBundle{
                                mesh: mesh.clone(),
                                material: material.clone(),
                                transform: event.dead_unit_data.4,
                                ..default()
                            })
                            .insert(UnitRemains{
                                number: remains_count.0,
                            }).insert(LOD{
                                detailed: (mesh, None, Some(material.clone())),
                                simplified: (unit_assets.vehicle_simplified_mesh.clone(), material),
                            });
                        }
                        _ => {}
                    }
                }

                tile_map.tiles.entry(event.dead_unit_data.0).or_insert_with(HashMap::new).entry(event.dead_unit_data.1.0)
                .or_insert_with(HashMap::new).remove(&event.dead_unit_data.3);

                if let Some(cover_entity) = event.dead_unit_data.2 {
                    event_writer.0.send(UnitNeedsToBeUncovered {
                        cover_entity: cover_entity,
                        unit_entity: event.dead_unit_data.3,
                    });
                }

                commands.entity(event.dead_unit_data.3).despawn_recursive();
        
                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitRemoved {
                            server_entity: event.dead_unit_data.3,
                            unit_data: (
                                event.dead_unit_data.0,
                                event.dead_unit_data.1.0,
                                event.dead_unit_data.1.1.clone(),
                            ),
                            should_spawn_corpse: event.dead_unit_data.5,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
        
                match event.dead_unit_data.1.1.0 {
                    CompanyTypes::Regular => {
                        production_queue.0.entry(event.dead_unit_data.0).or_insert_with(|| ProductionQueueObject{
                            regular_infantry_queue: HashMap::new(),
                            shock_infantry_queue: HashMap::new(),
                            vehicles_queue: HashMap::new(),
                            artillery_queue: HashMap::new(),
                            engineers_queue: HashMap::new(),
                        })
                        .regular_infantry_queue
                        .insert(
                            event.dead_unit_data.1.1.1,
                            (
                                event.dead_unit_data.1.1.2.clone(),
                                Entity::PLACEHOLDER,
                            ),
                        );
        
                        if let Some (platoon) = army.0.get_mut(&event.dead_unit_data.0).unwrap().regular_squads.get_mut(&(
                            event.dead_unit_data.1.1.1.0,
                            event.dead_unit_data.1.1.1.1,
                            event.dead_unit_data.1.1.1.2,
                            event.dead_unit_data.1.1.1.3,
                            event.dead_unit_data.1.1.1.4,
                        )) {
                            if event.dead_unit_data.1.1.1.5 == 0 {
                                platoon.0.0.0.remove(&event.dead_unit_data.3);
                            } else {
                                platoon.0.0.1.remove(&event.dead_unit_data.3);
                            }
                        }
                    },
                    CompanyTypes::Shock => {
                        production_queue.0.entry(event.dead_unit_data.0).or_insert_with(|| ProductionQueueObject{
                            regular_infantry_queue: HashMap::new(),
                            shock_infantry_queue: HashMap::new(),
                            vehicles_queue: HashMap::new(),
                            artillery_queue: HashMap::new(),
                            engineers_queue: HashMap::new(),
                        })
                        .shock_infantry_queue.insert(
                            event.dead_unit_data.1.1.1,
                            (
                                event.dead_unit_data.1.1.2.clone(),
                                Entity::PLACEHOLDER,
                            ),
                        );
        
                        if let Some (platoon) = army.0.get_mut(&event.dead_unit_data.0).unwrap().shock_squads.get_mut(&(
                            event.dead_unit_data.1.1.1.0,
                            event.dead_unit_data.1.1.1.1,
                            event.dead_unit_data.1.1.1.2,
                            event.dead_unit_data.1.1.1.3,
                            event.dead_unit_data.1.1.1.4,
                        )) {
                            if event.dead_unit_data.1.1.1.5 == 0 {
                                platoon.0.0.0.remove(&event.dead_unit_data.3);
                            } else {
                                platoon.0.0.1.remove(&event.dead_unit_data.3);
                            }
                        }
                    },
                    CompanyTypes::Armored => {
                        production_queue.0.entry(event.dead_unit_data.0).or_insert_with(|| ProductionQueueObject{
                            regular_infantry_queue: HashMap::new(),
                            shock_infantry_queue: HashMap::new(),
                            vehicles_queue: HashMap::new(),
                            artillery_queue: HashMap::new(),
                            engineers_queue: HashMap::new(),
                        })
                        .vehicles_queue.insert(
                            event.dead_unit_data.1.1.1,
                            (
                                event.dead_unit_data.1.1.2.clone(),
                                Entity::PLACEHOLDER,
                            ),
                        );
        
                        if let Some (platoon) = army.0.get_mut(&event.dead_unit_data.0).unwrap().armored_squads.get_mut(&(
                            event.dead_unit_data.1.1.1.0,
                            event.dead_unit_data.1.1.1.1,
                            event.dead_unit_data.1.1.1.2,
                            event.dead_unit_data.1.1.1.3,
                            event.dead_unit_data.1.1.1.4,
                        )) {
                            platoon.0.0.remove(&event.dead_unit_data.3);
                        }
                    },
                    CompanyTypes::Artillery => {
                        production_queue.0.entry(event.dead_unit_data.0).or_insert_with(|| ProductionQueueObject{
                            regular_infantry_queue: HashMap::new(),
                            shock_infantry_queue: HashMap::new(),
                            vehicles_queue: HashMap::new(),
                            artillery_queue: HashMap::new(),
                            engineers_queue: HashMap::new(),
                        })
                        .artillery_queue.insert(
                            event.dead_unit_data.1.1.1,
                            (
                                event.dead_unit_data.1.1.2.clone(),
                                Entity::PLACEHOLDER,
                            ),
                        );
        
                        if let Some(artillery) = army.0.get_mut(&event.dead_unit_data.0).unwrap().artillery_units.0.get_mut(&event.dead_unit_data.1.1.1.6){
                            artillery.0.0 = None;
                        }
                    },
                    CompanyTypes::Engineer => {
                        production_queue.0.entry(event.dead_unit_data.0).or_insert_with(|| ProductionQueueObject{
                            regular_infantry_queue: HashMap::new(),
                            shock_infantry_queue: HashMap::new(),
                            vehicles_queue: HashMap::new(),
                            artillery_queue: HashMap::new(),
                            engineers_queue: HashMap::new(),
                        })
                        .engineers_queue.insert(
                            event.dead_unit_data.1.1.1,
                            (
                                event.dead_unit_data.1.1.2.clone(),
                                Entity::PLACEHOLDER,
                            ),
                        );
        
                        if let Some(engineer) = army.0.get_mut(&event.dead_unit_data.0).unwrap().engineers.get_mut(&event.dead_unit_data.1.1.1.6){
                            engineer.0.0 = None;
                        }
                    },
                    CompanyTypes::None => {},
                }
            }
        }
    }
}

pub fn unit_production_buttons_handler(
    mut buildings_q: Query<(Entity, &mut UnitProductionBuildingComponent), With<SelectedBuilding>>,
    mut event_reader: EventReader<ProductionButtonPressed>,
    mut commands: Commands,
){
    // for event in event_reader.read() {
    //     for (building_entity, mut building_component) in buildings_q.iter_mut() {
    //         if let Some(unit_to_build) = building_component.available_to_build.clone().get(&event.data) {
    //             building_component.build_order.push(unit_to_build.clone());
    //             commands.entity(building_entity).try_insert(NeedToProduce);
    //         }
    //     }
    // }
}

pub fn production_manager(
    mut units_producers_q: Query<&mut UnitProductionBuildingComponent>,
    mut production_queue: ResMut<ProductionQueue>,
    army: Res<Armies>,
    mut event_reader: EventReader<ProductionStateChanged>,
    game_stage: Res<GameStage>,
    //player_data: Res<PlayerData>,
    mut event_writer: EventWriter<GameStartedEvent>,
    network_status: Res<NetworkStatus>,
){
    for event in event_reader.read() {
        if event.is_allowed {
            let mut add_amount;
            for platoon in army.0.get(&event.team).unwrap().regular_squads.iter() {
                add_amount = platoon.1.0.0.0.capacity() - platoon.1.0.0.0.len();

                for i in 0..add_amount as i32 {
                    production_queue.0.get_mut(&event.team).unwrap().regular_infantry_queue.insert(
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
                    production_queue.0.get_mut(&event.team).unwrap().regular_infantry_queue.insert(
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

            for platoon in army.0.get(&event.team).unwrap().shock_squads.iter() {
                add_amount = platoon.1.0.0.0.capacity() - platoon.1.0.0.0.len();

                for i in 0..add_amount as i32 {
                    production_queue.0.get_mut(&event.team).unwrap().shock_infantry_queue.insert(
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
                    production_queue.0.get_mut(&event.team).unwrap().shock_infantry_queue.insert(
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

            for platoon in army.0.get(&event.team).unwrap().armored_squads.iter() {
                add_amount = platoon.1.0.0.capacity() - platoon.1.0.0.len();

                for i in 0..add_amount as i32 {
                    production_queue.0.get_mut(&event.team).unwrap().vehicles_queue.insert(
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

            for artillery_unit in army.0.get(&event.team).unwrap().artillery_units.0.iter() {
                if artillery_unit.1.0.0 == None {
                    production_queue.0.get_mut(&event.team).unwrap().artillery_queue.insert((0,0,0,0,0,0, *artillery_unit.0), (artillery_unit.1.0.1.clone(), Entity::PLACEHOLDER));
                }
            }

            for engineer in army.0.get(&event.team).unwrap().engineers.iter() {
                if engineer.1.0.0 == None {
                    production_queue.0.get_mut(&event.team).unwrap().engineers_queue.insert((0,0,0,0,0,0, *engineer.0), (engineer.1.0.1.clone(), Entity::PLACEHOLDER));
                }
            }
        } else {
            production_queue.0.get_mut(&event.team).unwrap().regular_infantry_queue.clear();
            production_queue.0.get_mut(&event.team).unwrap().shock_infantry_queue.clear();
            production_queue.0.get_mut(&event.team).unwrap().vehicles_queue.clear();
            production_queue.0.get_mut(&event.team).unwrap().artillery_queue.clear();
            production_queue.0.get_mut(&event.team).unwrap().engineers_queue.clear();

            for mut units_producer in units_producers_q.iter_mut() {
                units_producer.build_order.clear();
                units_producer.elapsed_time = 0;
            }
        }

        if let GameStages::ArmySetup = game_stage.0 {
            if !matches!(network_status.0, NetworkStatuses::Host) {
                event_writer.send(GameStartedEvent);
            }
        }
    }
}

pub fn unit_uncovering_system(
    mut event_reader: EventReader<UnitNeedsToBeUncovered>,
    mut covers_q: Query<(Entity, &mut CoverComponent), With<CoverComponent>>,
){
    for event in event_reader.read() {
        if let Ok(mut cover) = covers_q.get_mut(event.cover_entity) {
            cover.1.units_inside.remove(&event.unit_entity);
        }
    }

}

pub fn settlements_placement_system (
    mut commands: Commands,
    ui_button_nodes: Res<UiButtonNodes>,
    mut remaining_settlements: ResMut<SettlementsLeft>,
    placed_settlements: Query<(&SettlementComponent, &Transform), Without<DisplayedModelHolder>>,
    mut displayed_models_and_terrain_q: (
        Query<(Entity, &mut Transform), With<DisplayedModelHolder>>,
        Query<Entity, (With<Terrain>, Without<DisplayedModelHolder>)>,
    ),
    mut materials_and_models: (
        Res<Assets<StandardMaterial>>,
        ResMut<InstancedMaterials>,
        ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
        Res<BuildingsAssets>,
    ),
    cursor_ray_and_rapier_context: (
        Res<CursorRay>,
        Res<RapierContext>,
    ),
    mut raycast: Raycast,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut event_writer: (
        EventWriter<AllSettlementsPlaced>,
        EventWriter<DeleteTemporaryObjects>,
    ),
    mut game_stage: ResMut<GameStage>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
    clients: Res<ClientList>,
    mut player_data: ResMut<PlayerData>,
) {
    if let GameStages::SettlementsSetup = game_stage.0 {
        if !remaining_settlements.0.is_empty() {
            if remaining_settlements.0.len() > VILLAGES_COUNT as usize {
                commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();

                commands.entity(ui_button_nodes.middle_upper_node_row).with_children(|parent| {
                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_button_nodes.middle_upper_node_width),
                            height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                            margin: UiRect {
                                left: Val::Px(ui_button_nodes.margin),
                                right: Val::Px(ui_button_nodes.margin),
                                top: Val::Px(ui_button_nodes.margin),
                                bottom: Val::Px(ui_button_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: format!("Cities to place left: {0}", remaining_settlements.0.len() - VILLAGES_COUNT as usize),
                                    style: TextStyle {
                                        font_size: 20.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default() 
                            },
                            ..default()
                        });
                    });
                });
            } else {
                commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();
                
                commands.entity(ui_button_nodes.middle_upper_node_row).with_children(|parent| {
                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_button_nodes.middle_upper_node_width),
                            height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                            margin: UiRect {
                                left: Val::Px(ui_button_nodes.margin),
                                right: Val::Px(ui_button_nodes.margin),
                                top: Val::Px(ui_button_nodes.margin),
                                bottom: Val::Px(ui_button_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: format!("Villages to place left: {0}", remaining_settlements.0.len()),
                                    style: TextStyle {
                                        font_size: 20.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default() 
                            },
                            ..default()
                        });
                    });
                });
            }

            let mut terrain_entity = Entity::PLACEHOLDER;

            if let Ok(terrain) = displayed_models_and_terrain_q.1.get_single() {
                terrain_entity = terrain;
            }

            let mut ray_hit = Vec3::ZERO;
            if let Some(cursor_ray) = **cursor_ray_and_rapier_context.0 {
                let hits = raycast.cast_ray(cursor_ray, &RaycastSettings{
                    filter: &move |entity| entity == terrain_entity,
                    ..default()
                });
    
                if hits.len() > 0 {
                    ray_hit = hits[0].1.position();
                }
            }

            let mut is_forbidden = false;

            let mut shape_position = ray_hit;

            shape_position.y += 5.5;

            let intersections = cursor_ray_and_rapier_context.1.intersection_with_shape(
                shape_position,
                Quat::IDENTITY,
                &Collider::cylinder(5., remaining_settlements.0[0].0.settlement_size),
                QueryFilter::default(),
            );

            if intersections.is_some() {
                is_forbidden = true;
            }

            if ray_hit.y > 5. {
                is_forbidden = true;
            }

            for settlement in placed_settlements.iter() {
                if ray_hit.xz().distance(settlement.1.translation.xz()) < settlement.0.0.buffer_zone_size {
                    is_forbidden = true;
                }
            }

            if player_data.team == 1 {
                if
                ray_hit.x > WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.x < -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.z > -ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.z < -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS
                {
                    is_forbidden = true;
                }
            } else {
                if
                ray_hit.x > WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.x < -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.z < ALLOWED_DISTANCE_FROM_BORDERS ||
                ray_hit.z > WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS
                {
                    is_forbidden = true;
                }
            }

            if displayed_models_and_terrain_q.0.is_empty() {
                commands.spawn(MaterialMeshBundle{
                    mesh: remaining_settlements.0[0].1.mesh.clone(),
                    material: remaining_settlements.0[0].1.material.clone(),
                    transform: Transform::from_translation(Vec3::new(ray_hit.x, ray_hit.y + 5., ray_hit.z)),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            } else {
                for mut displayed_model in displayed_models_and_terrain_q.0.iter_mut() {
                    displayed_model.1.translation = Vec3::new(ray_hit.x, ray_hit.y + 5., ray_hit.z);

                    if is_forbidden {
                        commands.entity(displayed_model.0).insert(ForbiddenBlueprint);
                    } else {
                        commands.entity(displayed_model.0).remove::<ForbiddenBlueprint>();
                    }
                }
            }

            if mouse_buttons.just_pressed(MouseButton::Left) {    
                if !is_forbidden {
                    for displayed_model in displayed_models_and_terrain_q.0.iter() {
                        commands.entity(displayed_model.0).despawn();
                    }

                    let color;
                    if remaining_settlements.0[0].0.team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let angle = 45.0_f32.to_radians();

                    match network_status.0 {
                        NetworkStatuses::SinglePlayer => {
                            let material;

                            if let Some(mat) =
                            materials_and_models.1.team_materials.get(&(materials_and_models.3.town_hall.0.id(), remaining_settlements.0[0].0.team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials_and_models.0.get(materials_and_models.3.town_hall.1.id()) {
                                    material = materials_and_models.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials_and_models.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials_and_models.1.team_materials.insert((materials_and_models.3.town_hall.0.id(), remaining_settlements.0[0].0.team), material.clone());
                            }
                            
                            commands.spawn(MaterialMeshBundle{
                                mesh: materials_and_models.3.town_hall.0.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(ray_hit).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            })
                            .insert(SettlementComponent(remaining_settlements.0[0].0.clone()));

                            commands.spawn(CircleHolder(vec![
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.settlement_size,
                                    outer_radius: remaining_settlements.0[0].0.settlement_size + 1.,
                                    highlight_color: Vec4::new(1., 1., 1., 1.),
                                },
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.buffer_zone_size,
                                    outer_radius: remaining_settlements.0[0].0.buffer_zone_size + 1.,
                                    highlight_color: Vec4::new(1., 0., 0., 1.),
                                },
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.max_road_connection_distance,
                                    outer_radius: remaining_settlements.0[0].0.max_road_connection_distance + 1.,
                                    highlight_color: Vec4::new(0., 1., 0., 1.),
                                },
                            ]))
                            .insert(TemporaryObject);
                        },
                        NetworkStatuses::Host => {
                            let material;

                            if let Some(mat) =
                            materials_and_models.1.team_materials.get(&(materials_and_models.3.town_hall.0.id(), remaining_settlements.0[0].0.team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials_and_models.0.get(materials_and_models.3.town_hall.1.id()) {
                                    material = materials_and_models.2.add(ExtendedMaterial {
                                        base: original.clone(),
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                } else {
                                    material = materials_and_models.2.add(ExtendedMaterial {
                                        base: StandardMaterial{
                                            ..default()
                                        },
                                        extension: TeamMaterialExtension {
                                            team_color: color,
                                        },
                                    });
                                }

                                materials_and_models.1.team_materials.insert((materials_and_models.3.town_hall.0.id(), remaining_settlements.0[0].0.team), material.clone());
                            }
                            
                            let new_settlement = commands.spawn(MaterialMeshBundle{
                                mesh: materials_and_models.3.town_hall.0.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(ray_hit).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            })
                            .insert(SettlementComponent(remaining_settlements.0[0].0.clone()))
                            .id();

                            commands.spawn(CircleHolder(vec![
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.settlement_size,
                                    outer_radius: remaining_settlements.0[0].0.settlement_size + 1.,
                                    highlight_color: Vec4::new(1., 1., 1., 1.),
                                },
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.buffer_zone_size,
                                    outer_radius: remaining_settlements.0[0].0.buffer_zone_size + 1.,
                                    highlight_color: Vec4::new(1., 0., 0., 1.),
                                },
                                CircleData{
                                    circle_center: ray_hit.xz(),
                                    inner_radius: remaining_settlements.0[0].0.max_road_connection_distance,
                                    outer_radius: remaining_settlements.0[0].0.max_road_connection_distance + 1.,
                                    highlight_color: Vec4::new(0., 1., 0., 1.),
                                },
                            ]))
                            .insert(TemporaryObject);

                            let mut channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::SettlementPlaced {
                                    settlement: remaining_settlements.0[0].0.clone(),
                                    position: ray_hit,
                                    server_entity: new_settlement,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        },
                        NetworkStatuses::Client => {
                            let mut channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::SettlementPlacementRequest {
                                    settlement: remaining_settlements.0[0].0.clone(),
                                    position: ray_hit,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        },
                    }

                    remaining_settlements.0.remove(0);
                }
            }
        } else {
            commands.spawn(LineHolder(vec![
                LineData{
                    line_start: Vec2::new(WORLD_SIZE / 2., -ALLOWED_DISTANCE_FROM_BORDERS),
                    line_end: Vec2::new(-WORLD_SIZE / 2., -ALLOWED_DISTANCE_FROM_BORDERS),
                    line_width: 5.,
                    highlight_color: Vec4::new(0., 1., 1., 1.),
                },
            ]))
            .insert(DeleteAfterStart);

            match network_status.0 {
                NetworkStatuses::SinglePlayer => {
                    event_writer.0.send(AllSettlementsPlaced);
                    event_writer.1.send(DeleteTemporaryObjects);
                    game_stage.0 = GameStages::BuildingsSetup;
                },
                NetworkStatuses::Host => {
                    event_writer.1.send(DeleteTemporaryObjects);
                    player_data.is_all_settlements_placed = true;

                    commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();

                    commands.entity(ui_button_nodes.middle_upper_node_row).with_children(|parent| {
                        parent.spawn(ButtonBundle{
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(ui_button_nodes.middle_upper_node_width),
                                height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                                margin: UiRect {
                                    left: Val::Px(ui_button_nodes.margin),
                                    right: Val::Px(ui_button_nodes.margin),
                                    top: Val::Px(ui_button_nodes.margin),
                                    bottom: Val::Px(ui_button_nodes.margin),
                                },
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        })
                        .with_children(|button_parent| {
                            button_parent.spawn(TextBundle {
                                text: Text{
                                    sections: vec![TextSection {
                                        value: "All settlements placed".to_string(),
                                        style: TextStyle {
                                            font_size: 20.,
                                            ..default()
                                        },
                                        ..default()
                                    }],
                                    justify: JustifyText::Center,
                                    ..default() 
                                },
                                ..default()
                            });
                        });
                    });
                },
                NetworkStatuses::Client => {
                    event_writer.1.send(DeleteTemporaryObjects);
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::AllSettlementsPlaced){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }

                    commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();

                    commands.entity(ui_button_nodes.middle_upper_node_row).with_children(|parent| {
                        parent.spawn(ButtonBundle{
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(ui_button_nodes.middle_upper_node_width),
                                height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                                margin: UiRect {
                                    left: Val::Px(ui_button_nodes.margin),
                                    right: Val::Px(ui_button_nodes.margin),
                                    top: Val::Px(ui_button_nodes.margin),
                                    bottom: Val::Px(ui_button_nodes.margin),
                                },
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        })
                        .with_children(|button_parent| {
                            button_parent.spawn(TextBundle {
                                text: Text{
                                    sections: vec![TextSection {
                                        value: "All settlements placed".to_string(),
                                        style: TextStyle {
                                            font_size: 20.,
                                            ..default()
                                        },
                                        ..default()
                                    }],
                                    justify: JustifyText::Center,
                                    ..default() 
                                },
                                ..default()
                            });
                        });
                    });
                },
            }
        }
    }
}

pub fn apartments_generation_system(
    mut event_reader: EventReader<AllSettlementsPlaced>,
    mut settlements_q: Query<(Entity, &Transform, &mut SettlementComponent), With<SettlementComponent>>,
    buildings_assets: Res<BuildingsAssets>,
    mut tile_map: ResMut<UnitsTileMap>,
    mut commands: Commands,
    mut event_writer:(
        //EventWriter<UnsentServerMessage>,
        EventWriter<AllApartmentsPlaced>,
    ),
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for _event in event_reader.read() {
        let mut rng = rand::thread_rng();
        for mut settlement in settlements_q.iter_mut() {
            let mut placeds: Vec<Vec3> = Vec::new();

            for _i in 0..500 {
                let center = settlement.1.translation;
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                let mut distance = rng.gen_range(0.0..1.0).sqrt() * settlement.2.0.settlement_size;

                if distance < 40. {
                    distance = 40.;
                }
            
                let x = center.x + distance * theta.cos();
                let z = center.z + distance * theta.sin();

                let position = Vec3::new(x, center.y, z);
                let mut success = true;
                for placed in placeds.iter(){
                    if placed.xz().distance(position.xz()) <= 40. {
                        success = false;
                        break;
                    }
                }

                if success {
                    let color;
                    if settlement.2.0.team == 1 {
                        color = Color::srgb(0., 0., 1.);
                    } else {
                        color = Color::srgb(1., 0., 0.);
                    }

                    let rand = rng.gen_range(1..=4);

                    let angle;

                    match rand {
                        1 => {
                            angle = 45.0_f32.to_radians();
                        }
                        2 => {
                            angle = 135.0_f32.to_radians();
                        }
                        3 => {
                            angle = 225.0_f32.to_radians();
                        }
                        _ => {
                            angle = 315.0_f32.to_radians();
                        }
                    }

                    let new_apartment_tile = ((position.x / TILE_SIZE) as i32, (position.z / TILE_SIZE) as i32);

                    let new_apartment = commands.spawn(MaterialMeshBundle{
                        mesh: buildings_assets.apartment.0.clone(),
                        material: buildings_assets.apartment.1.clone(),
                        transform: Transform::from_translation(position).with_rotation(Quat::from_rotation_y(angle)),
                        ..default()
                    })
                    //.insert(Collider::cuboid(5., 5., 5.))
                    //.insert(NavMeshAffector)
                    .insert(ApartmentHouse)
                    .insert(CombatComponent{
                        team: settlement.2.0.team,
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
                        points: vec![position, position, position, position, position, position, position, position, position, position],
                        units_inside: HashSet::new(),
                    })
                    .id();

                    settlement.2.0.active_apartments.push((new_apartment, position, angle));
                    placeds.push(position);

                    tile_map.tiles.entry(settlement.2.0.team).or_insert_with(HashMap::new).entry(new_apartment_tile)
                    .or_insert_with(HashMap::new).insert(new_apartment, (position, UnitTypes::None));

                    if placeds.len() >= settlement.2.0.settlement_size as usize {
                        break;
                    }

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ApartmentGenerated {
                                team: settlement.2.0.team,
                                server_entity: new_apartment,
                                position: position,
                                angle: angle,
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

        event_writer.0.send(AllApartmentsPlaced);
    }
}

pub fn roads_generation_system_legacy(
    mut event_reader: EventReader<AllApartmentsPlaced>,
    mut settlements_q: Query<(Entity, &Transform, &mut SettlementComponent), With<SettlementComponent>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    nav_mesh_settings: Res<NavMeshSettings>,
    nav_mesh: Res<NavMesh>,
    mut commands: Commands,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<AllRoadsGenerated>,
    ),
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for _event in event_reader.read() {
        let mut roads: Vec<((Entity, Vec<Vec3>, Vec3), (Entity, Entity))> = Vec::new();

        for settlement1 in settlements_q.iter() {
            let mut is_atleast_one_road_built = false;
            let mut nearest_settlement = ((Entity::PLACEHOLDER, Vec3::ZERO), f32::INFINITY);

            for settlement2 in settlements_q.iter() {
                if settlement1.0 != settlement2.0 {
                    let distance_between_settlements = settlement1.1.translation.xz().distance(settlement2.1.translation.xz());
                    if distance_between_settlements <= settlement1.2.0.max_road_connection_distance {
                        is_atleast_one_road_built = true;

                        let road_center = (
                            Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z) +
                            Vec3::new(settlement2.1.translation.x, 0.01, settlement2.1.translation.z)
                        ) / 2.;
                        let road_start = Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z);
                        let road_end = Vec3::new(settlement2.1.translation.x, 0.01, settlement2.1.translation.z);
                        let mut road_points: Vec<Vec3> = Vec::new();

                        if let Ok(nav_mesh) = nav_mesh.get().read() {
                            match find_polygon_path(
                                &nav_mesh,
                                &nav_mesh_settings,
                                road_start,
                                road_end,
                                None,
                                Some(&[1.0, 1.0]),
                            ) {
                                Ok(path) => {
                                    match perform_string_pulling_on_path(&nav_mesh, road_start, road_end, &path) {
                                        Ok(string_path) => {
                                            road_points = string_path;
                                        }
                                        Err(error) => error!("Error with string path: {:?}", error),
                                    };
                                }
                                Err(error) => error!("Error with pathfinding: {:?}", error),
                            }
                        }

                        if road_points.len() > 1 {
                            let mut road_transform = Transform::from_translation(road_center);
                            road_transform.rotate(Quat::from_rotation_arc(Vec3::Z, (road_start - road_end).normalize()));
    
                            let raod_mesh = create_curved_mesh(
                                5.,
                                5.,
                                road_points.clone(),
                                -1.2,
                                &Transform::from_translation(road_center),
                            );
    
                            let new_road = commands.spawn(MaterialMeshBundle{
                                mesh: meshes.add(raod_mesh.clone()),
                                material: materials.add(Color::srgb(0.5, 0.5, 0.5)).into(),
                                transform: Transform::from_translation(road_center),
                                ..default()
                            })
                            .insert(Collider::from_bevy_mesh(&raod_mesh, &ComputedColliderShape::TriMesh).unwrap())
                            .insert(NavMeshAffector)
                            .insert(NavMeshAreaType(Some(Area(1))))
                            .insert(NotShadowCaster)
                            .id();
    
                            roads.push((
                                (
                                    new_road,
                                    road_points,
                                    road_center,
                                ),
                                (
                                    settlement1.0,
                                    settlement2.0,
                                ),
                            ));
                        }
                    } else {
                        if distance_between_settlements < nearest_settlement.1 {
                            nearest_settlement = ((settlement2.0, settlement2.1.translation), distance_between_settlements);
                        }
                    }
                }
            }

            if !is_atleast_one_road_built {
                let road_center = (
                    Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z) +
                    Vec3::new(nearest_settlement.0.1.x, 0.01, nearest_settlement.0.1.z)
                ) / 2.;
                let road_start = Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z);
                let road_end = Vec3::new(nearest_settlement.0.1.x, 0.01, nearest_settlement.0.1.z);

                let mut road_points: Vec<Vec3> = Vec::new();

                if let Ok(nav_mesh) = nav_mesh.get().read() {                    
                    match find_polygon_path(
                        &nav_mesh,
                        &nav_mesh_settings,
                        road_start,
                        road_end,
                        None,
                        Some(&[1.0, 1.0]),
                    ) {
                        Ok(path) => {
                            match perform_string_pulling_on_path(&nav_mesh, road_start, road_end, &path) {
                                Ok(string_path) => {
                                    road_points = string_path;
                                }
                                Err(error) => error!("Error with string path: {:?}", error),
                            };
                        }
                        Err(error) => error!("Error with pathfinding: {:?}", error),
                    }
                }

                if road_points.len() > 1 {
                    let mut road_transform = Transform::from_translation(road_center);
                    road_transform.rotate(Quat::from_rotation_arc(Vec3::Z, (road_start - road_end).normalize()));
    
                    let raod_mesh = create_curved_mesh(
                        5.,
                        5.,
                        road_points.clone(),
                        -1.2,
                        &Transform::from_translation(road_center),
                    );
    
                    let new_road = commands.spawn(MaterialMeshBundle{
                        mesh: meshes.add(raod_mesh.clone()),
                        material: materials.add(Color::srgb(0.5, 0.5, 0.5)).into(),
                        transform: Transform::from_translation(road_center),
                        ..default()
                    })
                    .insert(Collider::from_bevy_mesh(&raod_mesh, &ComputedColliderShape::TriMesh).unwrap())
                    .insert(NavMeshAffector)
                    .insert(NavMeshAreaType(Some(Area(1))))
                    .insert(NotShadowCaster)
                    .id();
    
                    roads.push((
                        (
                            new_road,
                            road_points,
                            road_center,
                        ),
                        (
                            settlement1.0,
                            nearest_settlement.0.0,
                        ),
                    ));
                }
            }
        }

        let mut roads_to_delete: Vec<Entity> = Vec::new();
        for road in roads.iter() {
            if !roads_to_delete.contains(&road.0.0) {
                for another_road in roads.iter() {
                    if !roads_to_delete.contains(&another_road.0.0) &&
                    road.0.1[0].xz() == another_road.0.1[another_road.0.1.len() - 1].xz() &&
                    road.0.1[road.0.1.len() - 1].xz() == another_road.0.1[0].xz(){
                        commands.entity(another_road.0.0).despawn();
                        roads_to_delete.push(another_road.0.0);
                    }
                }
            }
        }

        for road in roads_to_delete.iter() {
            roads.retain(|r| r.0.0 != *road);
        }

        for road in roads.iter() {
            if let Ok(mut settlement) = settlements_q.get_mut(road.1.0) {
                settlement.2.0.connected_roads.push(road.0.0);
                settlement.2.0.connected_settlements.push(road.1.1);
            }

            if let Ok(mut settlement) = settlements_q.get_mut(road.1.1) {
                settlement.2.0.connected_roads.push(road.0.0);
                settlement.2.0.connected_settlements.push(road.1.0);
            }
        }

        let mut settlements_clusters: Vec<Vec<Entity>> = Vec::new();
        let mut last_cluster_index: i32 = -1;

        let settlements_count = settlements_q.iter().count();

        for settlement in settlements_q.iter() {
            if last_cluster_index == -1 {
                last_cluster_index = 0;
                settlements_clusters.push(Vec::new());
                settlements_clusters[0].push(settlement.0);
            }
            else if !settlements_clusters[last_cluster_index as usize].contains(&settlement.0) {
                settlements_clusters.push(Vec::new());
                last_cluster_index += 1;
            } else {
                continue;
            }

            let mut times_unaffected = 0;
            let mut settlements_to_check = vec![settlement];

            while times_unaffected < settlements_count {
                let settlements_to_check_clone = settlements_to_check.clone();
                settlements_to_check = Vec::new();
                for current_settlement in settlements_to_check_clone.iter(){
                    for connected_settlement_entity in current_settlement.2.0.connected_settlements.iter() {
                        if !settlements_clusters[last_cluster_index as usize].contains(connected_settlement_entity) {
                            settlements_clusters[last_cluster_index as usize].push(*connected_settlement_entity);
                        } else {
                            times_unaffected += 1;
                        }
    
                        if let Ok(connected_settlement) = settlements_q.get(*connected_settlement_entity) {
                            settlements_to_check.push(connected_settlement);
                        }
                    }
                }
            }
        }

        while settlements_clusters.len() > 1 {
            let mut clusters_to_connect = (0, 0);

            for (index, cluster) in settlements_clusters.iter().enumerate() {
                let mut nearest_settlement = Entity::PLACEHOLDER;
                let mut nearest_another_settlement = (f32::INFINITY, Entity::PLACEHOLDER, 0);
    
                for settlement_entity in cluster.iter() {
                    if let Ok(settlement) = settlements_q.get(*settlement_entity) {
                        for (another_index, another_cluster) in settlements_clusters.iter().enumerate() {
                            if index != another_index {
                                for another_settlement_entity in another_cluster.iter() {
                                    if let Ok(another_settlement) = settlements_q.get(*another_settlement_entity) {
                                        let current_distance = settlement.1.translation.distance(another_settlement.1.translation);
                                        if current_distance < nearest_another_settlement.0 {
                                            nearest_settlement = settlement.0;
                                            nearest_another_settlement.0 = current_distance;
                                            nearest_another_settlement.1 = another_settlement.0;
                                            nearest_another_settlement.2 = another_index;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
    
                if let Ok(settlement1) = settlements_q.get(nearest_settlement) {
                    if let Ok(settlement2) = settlements_q.get(nearest_another_settlement.1) {
                        let road_center = (
                            Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z) +
                            Vec3::new(settlement2.1.translation.x, 0.01, settlement2.1.translation.z)
                        ) / 2.;
                        let road_start = Vec3::new(settlement1.1.translation.x, 0.01, settlement1.1.translation.z);
                        let road_end = Vec3::new(settlement2.1.translation.x, 0.01, settlement2.1.translation.z);
    
                        let mut road_points: Vec<Vec3> = Vec::new();

                        if let Ok(nav_mesh) = nav_mesh.get().read() {                    
                            match find_polygon_path(
                                &nav_mesh,
                                &nav_mesh_settings,
                                road_start,
                                road_end,
                                None,
                                Some(&[1.0, 1.0]),
                            ) {
                                Ok(path) => {
                                    match perform_string_pulling_on_path(&nav_mesh, road_start, road_end, &path) {
                                        Ok(string_path) => {
                                            road_points = string_path;
                                        }
                                        Err(error) => error!("Error with string path: {:?}", error),
                                    };
                                }
                                Err(error) => error!("Error with pathfinding: {:?}", error),
                            }
                        }

                        if road_points.len() > 1 {
                            let mut road_transform = Transform::from_translation(road_center);
                            road_transform.rotate(Quat::from_rotation_arc(Vec3::Z, (road_start - road_end).normalize()));
    
                            let raod_mesh = create_curved_mesh(
                                5.,
                                5.,
                                road_points.clone(),
                                -1.2,
                                &Transform::from_translation(road_center),
                            );
    
                            let new_road = commands.spawn(MaterialMeshBundle{
                                mesh: meshes.add(raod_mesh.clone()),
                                material: materials.add(Color::srgb(0.5, 0.5, 0.5)).into(),
                                transform: Transform::from_translation(road_center),
                                ..default()
                            })
                            .insert(Collider::from_bevy_mesh(&raod_mesh, &ComputedColliderShape::TriMesh).unwrap())
                            .insert(NavMeshAffector)
                            .insert(NavMeshAreaType(Some(Area(1))))
                            .insert(NotShadowCaster)
                            .id();
        
                            roads.push((
                                (
                                    new_road,
                                    road_points,
                                    road_center,
                                ),
                                (
                                    settlement1.0,
                                    settlement2.0,
                                ),
                            ));
                        }
                    }
                }
                
                clusters_to_connect = (index, nearest_another_settlement.2);
            }

            for road in roads.iter() {
                if let Ok(mut settlement) = settlements_q.get_mut(road.1.0) {
                    settlement.2.0.connected_roads.push(road.0.0);
                    settlement.2.0.connected_settlements.push(road.1.1);
                }
    
                if let Ok(mut settlement) = settlements_q.get_mut(road.1.1) {
                    settlement.2.0.connected_roads.push(road.0.0);
                    settlement.2.0.connected_settlements.push(road.1.0);
                }
            }

            let cluster_clone = settlements_clusters[clusters_to_connect.1].clone();
            settlements_clusters[clusters_to_connect.0].extend(cluster_clone);
            settlements_clusters.remove(clusters_to_connect.1);
        }

        roads_to_delete = Vec::new();
        for road in roads.iter() {
            if !roads_to_delete.contains(&road.0.0) {
                for another_road in roads.iter() {
                    if !roads_to_delete.contains(&another_road.0.0) &&
                    road.0.1[0].xz() == another_road.0.1[another_road.0.1.len() - 1].xz() &&
                    road.0.1[road.0.1.len() - 1].xz() == another_road.0.1[0].xz(){
                        commands.entity(another_road.0.0).despawn();
                        roads_to_delete.push(another_road.0.0);
                    }
                }
            }
        }

        for road in roads_to_delete.iter() {
            roads.retain(|r| r.0.0 != *road);
        }

        event_writer.0.send(AllRoadsGenerated);

        if matches!(network_status.0, NetworkStatuses::Host) {
            for road in roads.iter(){
                let mut channel_id = 60;
                while channel_id <= 89 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::RoadGenerated {
                        road_points: road.0.1.clone(),
                        road_center: road.0.2,
                        server_entity: road.0.0,
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

pub fn resource_zones_generation_system (
    mut event_reader: EventReader<AllRoadsGenerated>,
    settlements_q: Query<(Entity, &Transform, &SettlementComponent), With<SettlementComponent>>,
    rapier_context: Res<RapierContext>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut commands: Commands,
){
    for event in event_reader.read() {
        let mut resource_zones_to_place = RESOURCE_ZONES_COUNT;
        let mut x_border_positive = WORLD_SIZE / 2. - 100.;
        let mut x_border_negative = -WORLD_SIZE / 2. + 100.;
        let mut z_border_positive = -WORLD_SIZE / 2. * 0.3;
        let mut z_border_negative = -WORLD_SIZE / 2. + 100.;
        let mut rng = rand::thread_rng();

        let mut placed_resource_zones: Vec<Vec3> = Vec::new();
        let resource_zone_size = 30.;
        while resource_zones_to_place > 0 {
            let x_rng = rng.gen_range(x_border_negative..x_border_positive);
            let z_rng = rng.gen_range(z_border_negative..z_border_positive);
            let spot = Vec3::new(x_rng, 0.1, z_rng);

            let mut is_forbidden = false;

            for settlement in settlements_q.iter() {
                if settlement.1.translation.distance(spot) <= settlement.2.0.settlement_size + resource_zone_size + 10. || spot.y > 10. {
                    is_forbidden = true;
                }
            }

            for zone in placed_resource_zones.iter() {
                if spot.distance(*zone) <= resource_zone_size * 10. {
                    is_forbidden = true;
                }
            }

            let intersections = rapier_context.intersection_with_shape(
                Vec3::new(spot.x, 51., spot.z),
                Quat::IDENTITY,
                &Collider::cylinder(50., resource_zone_size* 1.5),
                QueryFilter::default(),
            );

            if intersections.is_some() {
                is_forbidden = true;
            }

            if !is_forbidden {
                placed_resource_zones.push(spot);
                resource_zones_to_place -= 1;

                let zone = commands.spawn(Transform::from_translation(spot))
                .insert(ResourceZone{
                    zone_radius: resource_zone_size,
                    current_miners: HashMap::new(),
                })
                .insert(CircleHolder(vec![
                    CircleData{
                        circle_center: spot.xz(),
                        inner_radius: resource_zone_size,
                        outer_radius: resource_zone_size + 1.,
                        highlight_color: Vec4::new(0., 1., 0., 1.),
                    },
                ]))
                .id();

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ResourceZonePlaced {
                            position: spot,
                            server_entity: zone,
                        }) {
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        resource_zones_to_place = RESOURCE_ZONES_COUNT;
        x_border_positive = WORLD_SIZE / 2. - 100.;
        x_border_negative = -WORLD_SIZE / 2. + 100.;
        z_border_positive = WORLD_SIZE / 2. - 100.;
        z_border_negative = WORLD_SIZE / 2. * 0.3;

        while resource_zones_to_place > 0 {
            let x_rng = rng.gen_range(x_border_negative..x_border_positive);
            let z_rng = rng.gen_range(z_border_negative..z_border_positive);
            let spot = Vec3::new(x_rng, 0.1, z_rng);

            let mut is_forbidden = false;

            for settlement in settlements_q.iter() {
                if settlement.1.translation.distance(spot) <= settlement.2.0.settlement_size + resource_zone_size + 10. || spot.y > 10.  {
                    is_forbidden = true;
                }
            }

            for zone in placed_resource_zones.iter() {
                if spot.distance(*zone) <= resource_zone_size * 10. {
                    is_forbidden = true;
                }
            }

            let intersections = rapier_context.intersection_with_shape(
                Vec3::new(spot.x, 51., spot.z),
                Quat::IDENTITY,
                &Collider::cylinder(50., resource_zone_size* 1.5),
                QueryFilter::default(),
            );

            if intersections.is_some() {
                is_forbidden = true;
            }

            if !is_forbidden {
                placed_resource_zones.push(spot);
                resource_zones_to_place -= 1;

                let zone = commands.spawn(Transform::from_translation(spot))
                .insert(ResourceZone{
                    zone_radius: resource_zone_size,
                    current_miners: HashMap::new(),
                })
                .insert(CircleHolder(vec![
                    CircleData{
                        circle_center: spot.xz(),
                        inner_radius: resource_zone_size,
                        outer_radius: resource_zone_size + 1.,
                        highlight_color: Vec4::new(0., 1., 0., 1.),
                    },
                ]))
                .id();

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ResourceZonePlaced {
                            position: spot,
                            server_entity: zone,
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
}

pub fn create_ring(
    radius_inner: f32,
    radius_outer: f32,
    segments: usize,
) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;

        let outer_x = angle.cos() * radius_outer;
        let outer_z = angle.sin() * radius_outer;

        let inner_x = angle.cos() * radius_inner;
        let inner_z = angle.sin() * radius_inner;

        positions.push([outer_x, 0.0, outer_z]);
        positions.push([inner_x, 0.0, inner_z]);

        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);

        uvs.push([0.0, 0.0]);
        uvs.push([0.0, 0.0]);
    }

    for i in 0..segments {
        let next = (i + 1) % segments;

        indices.push((i * 2) as u32);
        indices.push((i * 2 + 1) as u32);
        indices.push((next * 2) as u32);

        indices.push((i * 2 + 1) as u32);
        indices.push((next * 2 + 1) as u32);
        indices.push((next * 2) as u32);
    }

    let mut mesh = Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList, bevy::render::render_asset::RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));

    mesh
}

pub fn temporary_objects_deletion_system(
    temp_objects_q: Query<Entity, With<TemporaryObject>>,
    mut commands: Commands,
    mut event_reader: EventReader<DeleteTemporaryObjects>,
){
    for _event in event_reader.read() {
        for temp_object in temp_objects_q.iter() {
            commands.entity(temp_object).despawn();
        }
    }
}

pub fn settlements_capturing_system (
    mut commands: Commands,
    mut tile_map: ResMut<UnitsTileMap>,
    mut settlements_q: Query<(Entity, &Transform, &mut SettlementComponent), With<SettlementComponent>>,
    mut apartments_q: Query<&mut CombatComponent, With<ApartmentHouse>>,
    logistic_units_q: Query<&LogisticUnitComponent>,
    buildings_assets: Res<BuildingsAssets>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut materials: (
        ResMut<Assets<StandardMaterial>>,
        ResMut<InstancedMaterials>,
        ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    ),
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    timer: Res<Time>,
    mut elapsed_time: Local<u128>,
){
    *elapsed_time += timer.delta().as_millis();

    if *elapsed_time >= 1000 {
        *elapsed_time = 0;

        let mut top_right_tile: (i32, i32);
        let mut bottom_left_tile: (i32, i32);
        let mut rows: i32;
        let mut columns: i32;
        let mut tile_to_scan: (i32, i32);

        for mut settlement in settlements_q.iter_mut() {
            top_right_tile = (
                ((settlement.1.translation.x + settlement.2.0.settlement_size) / TILE_SIZE) as i32,
                ((settlement.1.translation.z + settlement.2.0.settlement_size) / TILE_SIZE) as i32
            );

            bottom_left_tile = (
                ((settlement.1.translation.x - settlement.2.0.settlement_size) / TILE_SIZE) as i32,
                ((settlement.1.translation.z - settlement.2.0.settlement_size) / TILE_SIZE) as i32
            );

            tile_to_scan = bottom_left_tile;
            rows = top_right_tile.1 - bottom_left_tile.1;
            columns = top_right_tile.0 - bottom_left_tile.0;

            let mut ally_count = 0;
            let mut enemy_count = 0;
            let mut units_count = 0;

            let mut enemy_team = 0;

            let mut distance_to_target = 0.;

            for _row in 0..rows + 1 {
                for _column in 0..columns + 1 {
                    for team_tile_map in tile_map.tiles.iter_mut() {
                        for (unit_entity, (unit_position, unit_type)) in team_tile_map.1.entry(tile_to_scan)
                        .or_insert_with(HashMap::new) {
                            distance_to_target = settlement.1.translation.xz().distance(unit_position.xz());

                            if distance_to_target <= settlement.2.0.settlement_size && !matches!(unit_type, UnitTypes::None | UnitTypes::Building) && !logistic_units_q.get(*unit_entity).is_ok() {
                                if settlement.2.0.team == *team_tile_map.0 {
                                    ally_count += 1;
                                } else {
                                    enemy_count += 1;

                                    enemy_team = *team_tile_map.0;
                                }

                                units_count += 1;

                                if units_count > 100 {
                                    break;
                                }
                            }
                        }

                        units_count = 0;
                    }

                    tile_to_scan.0 += 1;
                }

                tile_to_scan.1 += 1;
                tile_to_scan.0 -= columns + 1;
            }

            if enemy_count as f32 > ally_count as f32 * 1.5 {
                commands.entity(settlement.0).insert(SettlementCaptureInProgress);

                let bar_size = ui_button_nodes.button_size * 0.75;

                let color;
                if settlement.2.0.team == 1 {
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
                        constrcution_entity: settlement.0,
                        max_width: bar_size,
                    });
                });
                
                settlement.2.0.elapsed_capture_time += 1000;

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::SettlementCaptureStarted {
                            settlement_server_entity: settlement.0,
                        }) {
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }

                if settlement.2.0.elapsed_capture_time >= settlement.2.0.time_to_capture {
                    commands.entity(settlement.0).remove::<SettlementCaptureInProgress>();

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::SettlementCaptureEnded {
                                settlement_server_entity: settlement.0,
                            }) {
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }

                    settlement.2.0.team = enemy_team;
                    settlement.2.0.elapsed_capture_time = 0;

                    let mut captured_apartments: Vec<Entity> = Vec::new();
                    for apartment_entity in settlement.2.0.active_apartments.iter() {
                        if let Ok(mut apartment) = apartments_q.get_mut(apartment_entity.0) {
                            captured_apartments.push(apartment_entity.0);
                            apartment.team = enemy_team;
                        }
                        
                        commands.entity(apartment_entity.0).insert(Visibility::Visible);
                    }

                    let color;
                    if enemy_team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }

                    let material;

                    if let Some(mat) =
                    materials.1.team_materials.get(&(buildings_assets.town_hall.0.id(), enemy_team)) {
                        material = mat.clone();
                    } else {
                        if let Some(original) = materials.0.get(buildings_assets.town_hall.1.id()) {
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

                        materials.1.team_materials.insert((buildings_assets.town_hall.0.id(), enemy_team), material.clone());
                    }

                    commands.entity(settlement.0).insert(material);

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::SettlementCaptured {
                                server_entity: settlement.0,
                                team: enemy_team,
                                captured_apartments: captured_apartments.clone(),
                            }) {
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            } else {
                commands.entity(settlement.0).remove::<SettlementCaptureInProgress>();
                settlement.2.0.elapsed_capture_time = 0;

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::SettlementCaptureEnded {
                            settlement_server_entity: settlement.0,
                        }) {
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Component)]
pub struct MaterialsDisplay {
    pub storage_entity: Entity,
    pub original_width: f32,
}

#[derive(Component)]
pub struct HumanResourcesDisplay {
    pub storage_entity: Entity,
    pub original_width: f32,
}

pub fn resources_amount_displays_processing_system(
    mut material_displays_q: Query<(&MaterialsDisplay, &mut Style, &Parent), (With<MaterialsDisplay>, Without<HumanResourcesDisplay>, Without<Children>)>,
    mut human_resource_displays_q: Query<(&HumanResourcesDisplay, &mut Style, &Parent), (With<HumanResourcesDisplay>, Without<MaterialsDisplay>, Without<Children>)>,
    mut display_bar_holders_q: Query<&mut Style, (With<Children>, Without<MaterialsDisplay>, Without<HumanResourcesDisplay>)>,
    storages_q: Query<(&Transform, &CombatComponent, Option<&MaterialsStorageComponent>, Option<&HumanResourceStorageComponent>)>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    ui_button_nodes: Res<UiButtonNodes>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
){
    if material_displays_q.is_empty() && human_resource_displays_q.is_empty() {return;}

    let camera = camera_q.iter().next().unwrap();

    if camera.1.translation().y > 500. {
        for material_display in material_displays_q.iter() {
            commands.entity(**material_display.2).insert(Visibility::Hidden);
        }

        for human_resource_display in human_resource_displays_q.iter() {
            commands.entity(**human_resource_display.2).insert(Visibility::Hidden);
        }

        return;
    }

    let bar_width = ui_button_nodes.button_size * 0.75;
    let bar_left_offset = bar_width / 2.;

    let bar_height = bar_width / 4.;

    for mut material_display in material_displays_q.iter_mut() {
        if let Ok(storage) = storages_q.get(material_display.0.storage_entity) {
            if storage.1.team != player_data.team {continue;}

            if let Some(material_storage) = storage.2 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, storage.0.translation) {
                    if let Ok(mut holder) = display_bar_holders_q.get_mut(**material_display.2) {
                        commands.entity(**material_display.2).insert(Visibility::Visible);

                        holder.left = Val::Px(viewport_point.x - bar_left_offset);
                        holder.top = Val::Px(viewport_point.y);

                        let width_modifier = material_storage.available_resources as f32 / material_storage.materials_storage_capacity as f32;
                        let width = material_display.0.original_width * width_modifier;

                        material_display.1.width = Val::Px(width);
                    }
                } else {
                    commands.entity(**material_display.2).insert(Visibility::Hidden);
                }
            }
        } else {
            commands.entity(**material_display.2).despawn_recursive();
        }
    }

    for mut human_resource_display in human_resource_displays_q.iter_mut() {
        if let Ok(storage) = storages_q.get(human_resource_display.0.storage_entity) {
            if storage.1.team != player_data.team {continue;}

            if let Some(human_resource_storage) = storage.3 {
                let mut top_offset_add = 0.;

                if let Some(_ms) = storage.2 {
                    top_offset_add = bar_height;
                }

                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, storage.0.translation) {
                    if let Ok(mut holder) = display_bar_holders_q.get_mut(**human_resource_display.2) {
                        commands.entity(**human_resource_display.2).insert(Visibility::Visible);

                        holder.left = Val::Px(viewport_point.x - bar_left_offset);
                        holder.top = Val::Px(viewport_point.y + top_offset_add);

                        let width_modifier = human_resource_storage.available_human_resources as f32 / human_resource_storage.human_resource_storage_capacity as f32;
                        let width = human_resource_display.0.original_width * width_modifier;

                        human_resource_display.1.width = Val::Px(width);
                    }
                } else {
                    commands.entity(**human_resource_display.2).insert(Visibility::Hidden);
                }
            }
        } else {
            commands.entity(**human_resource_display.2).despawn_recursive();
        }
    }
}

pub const CONSTRUCTION_PROGRESS_COLOR: Color = Color::srgba(1., 1., 0., 1.);

#[derive(Component)]
pub struct ConstructionProgressBar {
    pub constrcution_entity: Entity,
    pub max_width: f32,
}

pub fn construction_progress_displays_processing_system(
    mut progress_bars_q: Query<(&ConstructionProgressBar, &mut Style, &Parent), With<ConstructionProgressBar>>,
    mut progress_bar_holders_q: Query<(&mut Style, &Children), Without<ConstructionProgressBar>>,
    construction_sites_q: Query<(&Transform, &BuildingConstructionSite, &CombatComponent), (With<BuildingConstructionSite>, Without<DeconstructableBuilding>)>,
    deconstruction_sites_q: Query<(&Transform, &DeconstructableBuilding, &CombatComponent), (With<DeconstructableBuilding>, With<ToDeconstruct>, Without<BuildingConstructionSite>)>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    ui_button_nodes: Res<UiButtonNodes>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
){
    let camera = camera_q.iter().next().unwrap();

    if camera.1.translation().y > 500. {
        for progress_bar in progress_bars_q.iter() {
            commands.entity(**progress_bar.2).insert(Visibility::Hidden);
        }

        return;
    }

    let bar_width = ui_button_nodes.button_size * 0.75;
    let bar_left_offset = bar_width / 2.;

    for mut progress_bar in progress_bars_q.iter_mut() {
        if let Ok(site) = construction_sites_q.get(progress_bar.0.constrcution_entity) {
            if site.2.team != player_data.team {continue;}

            if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, site.0.translation) {
                if let Ok(mut holder) = progress_bar_holders_q.get_mut(**progress_bar.2) {
                    commands.entity(**progress_bar.2).insert(Visibility::Visible);

                    holder.0.left = Val::Px(viewport_point.x - bar_left_offset);
                    holder.0.top = Val::Px(viewport_point.y);

                    let fraction = 1. - site.1.build_power_remaining as f32 / site.1.build_power_total as f32;

                    progress_bar.1.width = Val::Px(progress_bar.0.max_width * fraction);
                }
            }
        } else if let Ok(building) = deconstruction_sites_q.get(progress_bar.0.constrcution_entity) {
            if building.2.team != player_data.team {continue;}

            if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, building.0.translation) {
                if let Ok(mut holder) = progress_bar_holders_q.get_mut(**progress_bar.2) {
                    commands.entity(**progress_bar.2).insert(Visibility::Visible);

                    holder.0.left = Val::Px(viewport_point.x - bar_left_offset);
                    holder.0.top = Val::Px(viewport_point.y);

                    let fraction = 1. - building.1.buildpower_to_deconstruct_remaining as f32 / building.1.buildpower_to_deconstruct_total as f32;

                    progress_bar.1.width = Val::Px(progress_bar.0.max_width * fraction);
                }
            }
        } else {
            commands.entity(**progress_bar.2).despawn_recursive();
        }
    }
}

#[derive(Resource)]
pub struct BuildingsDeletionStates{
    pub is_blueprints_deletion_active: bool,
    pub is_buildings_deletion_active: bool,
    pub is_buildings_deletion_cancelation_active: bool,
}

pub fn buildings_deletion_activation_system(
    mut event_reader: (
        EventReader<ActivateBlueprintsDeletionMode>,
        EventReader<ActivateBuildingsDeletionMode>,
        EventReader<ActivateBuildingsDeletionCancelationMode>,
    ),
    mut deletion_states: ResMut<BuildingsDeletionStates>,
    selection_node: Query<Entity, With<SelectionBox>>,
    mut unit_selection: ResMut<IsUnitSelectionAllowed>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    game_stage: Res<GameStage>,
    mut commands: Commands,
){
    for _event in event_reader.0.read() {
        if !matches!(game_stage.0, GameStages::GameStarted) {
            return;
        }

        let selection_box = selection_node.single();

        if deletion_states.is_blueprints_deletion_active {
            deletion_states.is_blueprints_deletion_active = false;

            unit_selection.0 = true;

            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
        } else {
            deletion_states.is_blueprints_deletion_active = true;
            deletion_states.is_buildings_deletion_active = false;
            deletion_states.is_buildings_deletion_cancelation_active = false;

            unit_selection.0 = false;
            
            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(1., 0., 0., 0.1).into()));
        }
    }

    for _event in event_reader.1.read() {
        if !matches!(game_stage.0, GameStages::GameStarted) {
            return;
        }

        let selection_box = selection_node.single();

        if deletion_states.is_buildings_deletion_active {
            deletion_states.is_buildings_deletion_active = false;

            unit_selection.0 = true;

            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
        } else {
            deletion_states.is_buildings_deletion_active = true;
            deletion_states.is_blueprints_deletion_active = false;
            deletion_states.is_buildings_deletion_cancelation_active = false;

            unit_selection.0 = false;

            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(1., 0., 0., 0.1).into()));
        }
    }

    for _event in event_reader.2.read() {
        if !matches!(game_stage.0, GameStages::GameStarted) {
            return;
        }

        let selection_box = selection_node.single();

        if deletion_states.is_buildings_deletion_cancelation_active {
            deletion_states.is_buildings_deletion_cancelation_active = false;

            unit_selection.0 = true;

            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
        } else {
            deletion_states.is_buildings_deletion_cancelation_active = true;
            deletion_states.is_blueprints_deletion_active = false;
            deletion_states.is_buildings_deletion_active = false;

            unit_selection.0 = false;

            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 0., 0.1).into()));
        }
    }

    if mouse_buttons.just_pressed(MouseButton::Right) {
        let selection_box = selection_node.single();

        deletion_states.is_blueprints_deletion_active = false;
        deletion_states.is_buildings_deletion_active = false;
        deletion_states.is_buildings_deletion_cancelation_active = false;

        unit_selection.0 = true;

        commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
    }
}

pub fn blueprints_deletion_system(
    mut deletion_states: ResMut<BuildingsDeletionStates>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    blueprints_q: Query<(Entity, &Transform, &BuildingBlueprint)>,
    selection_bounds: Res<SelectionBounds>,
    camera_q: Query<(&CameraComponent, &Transform, &Camera, &GlobalTransform)>,
    selection_node: Query<Entity, With<SelectionBox>>,
    mut unit_selection: ResMut<IsUnitSelectionAllowed>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
    clients: Res<ClientList>,
    entity_maps: Res<EntityMaps>,
){
    if deletion_states.is_blueprints_deletion_active && !selection_bounds.is_ui_hovered {
        if mouse_buttons.just_released(MouseButton::Left) {
            let min_x = selection_bounds.first_point.x.min(selection_bounds.second_point.x);
            let max_x = selection_bounds.first_point.x.max(selection_bounds.second_point.x);
            let min_y = selection_bounds.first_point.y.min(selection_bounds.second_point.y);
            let max_y = selection_bounds.first_point.y.max(selection_bounds.second_point.y);

            let camera = camera_q.single();

            for blueprint in blueprints_q.iter() {
                if blueprint.2.team != player_data.team {continue;}

                if let Some(screen_pos) = camera.2.world_to_viewport(camera.3, blueprint.1.translation) {
                    if
                    screen_pos.x >= min_x && screen_pos.x <= max_x &&
                    screen_pos.y >= min_y && screen_pos.y <= max_y {
                        match network_status.0 {
                            NetworkStatuses::Client => {
                                if let Some(server_entity) = entity_maps.client_to_server.get(&blueprint.0) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::DeleteUnspecifiedEntityRequest {
                                            entity: *server_entity,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                            NetworkStatuses::Host => {
                                commands.entity(blueprint.0).despawn();

                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                        server_entity: blueprint.0,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            _ => {
                                commands.entity(blueprint.0).despawn();
                            },
                        }
                    }
                }
            }

            deletion_states.is_blueprints_deletion_active = false;
            let selection_box = selection_node.single();
            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
            unit_selection.0 = true;
        }
    }
}

#[derive(Component)]
pub struct DeconstructableBuilding {
    pub team: i32,
    pub materials_spent: i32,
    pub buildpower_to_deconstruct_total: i32,
    pub buildpower_to_deconstruct_remaining: i32,
    pub deconstruction_distance: f32,
}

#[derive(Component)]
pub struct ToDeconstruct{
    pub team: i32,
    pub deconstructor_entity: Entity,
    pub progress_bar_entity: Entity,
    pub deconstruction_distance: f32,
}

pub fn buildings_deletion_system(
    mut deletion_states: ResMut<BuildingsDeletionStates>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    construction_sites_q: Query<(Entity, &Transform, &BuildingConstructionSite), (With<BuildingConstructionSite>, Without<DeconstructableBuilding>, Without<DontTouch>)>,
    buildings_q: Query<(Entity, &Transform, &DeconstructableBuilding), (With<DeconstructableBuilding>, Without<BuildingConstructionSite>)>,
    selection_bounds: Res<SelectionBounds>,
    camera_q: Query<(&CameraComponent, &Transform, &Camera, &GlobalTransform)>,
    selection_node: Query<Entity, With<SelectionBox>>,
    mut unit_selection: ResMut<IsUnitSelectionAllowed>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
    clients: Res<ClientList>,
    entity_maps: Res<EntityMaps>,
){
    if deletion_states.is_buildings_deletion_active {
        if mouse_buttons.just_released(MouseButton::Left) && !selection_bounds.is_ui_hovered  {
            let min_x = selection_bounds.first_point.x.min(selection_bounds.second_point.x);
            let max_x = selection_bounds.first_point.x.max(selection_bounds.second_point.x);
            let min_y = selection_bounds.first_point.y.min(selection_bounds.second_point.y);
            let max_y = selection_bounds.first_point.y.max(selection_bounds.second_point.y);

            let camera = camera_q.single();

            for construction_site in construction_sites_q.iter() {
                if construction_site.2.team != player_data.team {continue;}

                if let Some(screen_pos) = camera.2.world_to_viewport(camera.3, construction_site.1.translation) {
                    if
                    screen_pos.x >= min_x && screen_pos.x <= max_x &&
                    screen_pos.y >= min_y && screen_pos.y <= max_y {
                        match network_status.0 {
                            NetworkStatuses::Client => {
                                if let Some(server_entity) = entity_maps.client_to_server.get(&construction_site.0) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::DeconstructionRequest {
                                            entity: *server_entity,
                                            team: construction_site.2.team,
                                            deconstruction_distance: construction_site.2.build_distance,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                            NetworkStatuses::Host => {
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::DeconstructionAssigned {
                                        server_entity: construction_site.0,
                                        team: construction_site.2.team,
                                        deconstruction_distance: construction_site.2.build_distance,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }

                                commands.entity(construction_site.0).insert(ToDeconstruct{
                                    team: construction_site.2.team,
                                    deconstructor_entity: Entity::PLACEHOLDER,
                                    progress_bar_entity: Entity::PLACEHOLDER,
                                    deconstruction_distance: construction_site.2.build_distance,
                                });
                            },
                            _ => {
                                commands.entity(construction_site.0).insert(ToDeconstruct{
                                    team: construction_site.2.team,
                                    deconstructor_entity: Entity::PLACEHOLDER,
                                    progress_bar_entity: Entity::PLACEHOLDER,
                                    deconstruction_distance: construction_site.2.build_distance,
                                });
                            },
                        }
                    }
                }
            }

            for building in buildings_q.iter() {
                if building.2.team != player_data.team {continue;}

                if let Some(screen_pos) = camera.2.world_to_viewport(camera.3, building.1.translation) {
                    if
                    screen_pos.x >= min_x && screen_pos.x <= max_x &&
                    screen_pos.y >= min_y && screen_pos.y <= max_y {
                        match network_status.0 {
                            NetworkStatuses::Client => {
                                if let Some(server_entity) = entity_maps.client_to_server.get(&building.0) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::DeconstructionRequest {
                                            entity: *server_entity,
                                            team: building.2.team,
                                            deconstruction_distance: building.2.deconstruction_distance,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                            NetworkStatuses::Host => {
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::DeconstructionAssigned {
                                        server_entity: building.0,
                                        team: building.2.team,
                                        deconstruction_distance: building.2.deconstruction_distance,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }

                                commands.entity(building.0).insert(ToDeconstruct{
                                    team: building.2.team,
                                    deconstructor_entity: Entity::PLACEHOLDER,
                                    progress_bar_entity: Entity::PLACEHOLDER,
                                    deconstruction_distance: building.2.deconstruction_distance,
                                });
                            },
                            _ => {
                                commands.entity(building.0).insert(ToDeconstruct{
                                    team: building.2.team,
                                    deconstructor_entity: Entity::PLACEHOLDER,
                                    progress_bar_entity: Entity::PLACEHOLDER,
                                    deconstruction_distance: building.2.deconstruction_distance,
                                });
                            },
                        }
                    }
                }
            }

            deletion_states.is_buildings_deletion_active = false;
            let selection_box = selection_node.single();
            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
            unit_selection.0 = true;
        }
    }
}

pub fn buildings_deletion_cancelation_system(
    mut deletion_states: ResMut<BuildingsDeletionStates>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut deconstruction_sites_q: Query<(Entity, &Transform, &ToDeconstruct, Option<&mut DeconstructableBuilding>, Option<&BuildingConstructionSite>), With<ToDeconstruct>>,
    selection_bounds: Res<SelectionBounds>,
    camera_q: Query<(&CameraComponent, &Transform, &Camera, &GlobalTransform)>,
    selection_node: Query<Entity, With<SelectionBox>>,
    mut unit_selection: ResMut<IsUnitSelectionAllowed>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
    clients: Res<ClientList>,
    entity_maps: Res<EntityMaps>,
){
    if deletion_states.is_buildings_deletion_cancelation_active && !selection_bounds.is_ui_hovered  {
        if mouse_buttons.just_released(MouseButton::Left) {
            let min_x = selection_bounds.first_point.x.min(selection_bounds.second_point.x);
            let max_x = selection_bounds.first_point.x.max(selection_bounds.second_point.x);
            let min_y = selection_bounds.first_point.y.min(selection_bounds.second_point.y);
            let max_y = selection_bounds.first_point.y.max(selection_bounds.second_point.y);

            let camera = camera_q.single();

            for deconstruction_site in deconstruction_sites_q.iter_mut() {
                if deconstruction_site.2.team != player_data.team {continue;}

                if let Some(screen_pos) = camera.2.world_to_viewport(camera.3, deconstruction_site.1.translation) {
                    if
                    screen_pos.x >= min_x && screen_pos.x <= max_x &&
                    screen_pos.y >= min_y && screen_pos.y <= max_y {
                        match network_status.0 {
                            NetworkStatuses::Client => {
                                if let Some(server_entity) = entity_maps.client_to_server.get(&deconstruction_site.0) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::DeconstructionCancelationRequest {
                                            entity: *server_entity,
                                            position: deconstruction_site.1.translation,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                            NetworkStatuses::Host => {
                                commands.entity(deconstruction_site.0).remove::<ToDeconstruct>();

                                if let Some(mut building) = deconstruction_site.3 {
                                    building.buildpower_to_deconstruct_remaining = 0;
                                }

                                if let Some(construction_site) = deconstruction_site.4 {
                                    commands.entity(construction_site.current_builder).insert(BusyEngineer(
                                        EngineerActions::Construction((deconstruction_site.1.translation, deconstruction_site.0, construction_site.build_distance))
                                    ));
                                }

                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::DeconstructionCanceled {
                                        server_entity: deconstruction_site.0,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            },
                            _ => {
                                commands.entity(deconstruction_site.0).remove::<ToDeconstruct>();

                                if let Some(mut building) = deconstruction_site.3 {
                                    building.buildpower_to_deconstruct_remaining = 0;
                                }

                                if let Some(construction_site) = deconstruction_site.4 {
                                    commands.entity(construction_site.current_builder).insert(BusyEngineer(
                                        EngineerActions::Construction((deconstruction_site.1.translation, deconstruction_site.0, construction_site.build_distance))
                                    ));
                                }
                            },
                        }
                    }
                }
            }

            deletion_states.is_buildings_deletion_cancelation_active = false;
            let selection_box = selection_node.single();
            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));
            unit_selection.0 = true;
        }
    }
}

#[derive(Component)]
pub struct SwitchableBuilding(pub bool);

pub fn buildings_state_switcher(
    mut event_reader: EventReader<SwitchBuildingState>,
    mut buildings_q: Query<(Entity, &mut SwitchableBuilding)>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    for event in event_reader.read() {
        if let Ok(mut building) = buildings_q.get_mut(event.0) {
            building.1.0 = !building.1.0;

            for row in ui_button_nodes.left_bottom_node_rows.iter() {
                commands.entity(*row).despawn_descendants();
            }

            let color;
            let text;

            if building.1.0 {
                color = Color::srgba(0.1, 1., 0.1, 1.);
                text = "Off".to_string();
            } else {
                color = Color::srgba(1., 0.1, 0.1, 1.);
                text = "On".to_string();
            }

            commands.entity(ui_button_nodes.left_bottom_node_rows[0]).with_children(|parent|{
                parent.spawn(ButtonBundle{
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                        height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
                        margin: UiRect {
                            left: Val::Px(ui_button_nodes.margin),
                            right: Val::Px(ui_button_nodes.margin),
                            top: Val::Px(ui_button_nodes.margin),
                            bottom: Val::Px(ui_button_nodes.margin),
                        },
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: color.into(),
                    ..default()
                }).insert(ButtonAction{action: Actions::SwitchBuildingState(building.0)})
                .with_children(|button_parent| {
                    button_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: text,
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default() 
                        },
                        ..default()
                    });
                });
            });

            if matches!(network_status.0, NetworkStatuses::Client) {
                if let Some(server_entity) = entity_maps.client_to_server.get(&building.0) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::BuildingStateSwitchRequest {
                            entity: *server_entity,
                            state: building.1.0,
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
}

#[derive(Component)]
pub struct DontTouch;

pub fn rebuild_settlement_apartments_system (
    mut event_reader: EventReader<RebuildApartments>,
    mut settlements_q: Query<&mut SettlementComponent>,
    mut material_producers_q: Query<(&mut MaterialsProductionComponent, &CombatComponent)>,
    buildings_assets: Res<BuildingsAssets>,
    instanced_materials: Res<InstancedMaterials>,
    mut tile_map: ResMut<UnitsTileMap>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    for event in event_reader.read() {
        match network_status.0 {
            NetworkStatuses::Client => {
                if let Some(server_entity) = entity_maps.client_to_server.get(&event.0) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::ApartmentsRebuildingRequest {
                            entity: *server_entity,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            },
            _ => {
                if let Ok(mut settlement) = settlements_q.get_mut(event.0) {
                    if settlement.0.elapsed_capture_time > 0 {
                        return;
                    }

                    let team = settlement.0.team;
                    for apartment in settlement.0.active_apartments.iter_mut() {
                        if commands.get_entity(apartment.0).is_none() {
                            let mut total_materials = 0;

                            for material_producer in material_producers_q.iter() {
                                if team != material_producer.1.team {continue;}

                                total_materials += material_producer.0.available_materials;
                            }

                            if total_materials >= 5000 {
                                let mut remaining_resource_cost = 5000;

                                for mut material_producer in material_producers_q.iter_mut() {
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
                                    mesh: buildings_assets.apartment.0.clone(),
                                    material: instanced_materials.blue_transparent.clone(),
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

                                tile_map.tiles.entry(team).or_insert_with(HashMap::new).entry(new_construction_tile)
                                .or_insert_with(HashMap::new).insert(new_construction_site, (apartment.1, UnitTypes::None));

                                let bar_size = ui_button_nodes.button_size * 0.75;

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

                                if matches!(network_status.0, NetworkStatuses::Host) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = server
                                        .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ApartmentConstructionSitePlaced {
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
                }
            },
        }
    }
}

pub fn apartments_rebuilding_system(
    mut settlements_q: Query<&mut SettlementComponent>,
    mut construction_sites_q: Query<(Entity, &mut BuildingConstructionSite)>,
    buildings_assets: Res<BuildingsAssets>,
    mut tile_map: ResMut<UnitsTileMap>,
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    *elapsed_time += time.delta().as_millis();

    if *elapsed_time >= 500 {
        *elapsed_time = 0;

        for mut settlement in settlements_q.iter_mut() {
            let team = settlement.0.team;

            for apartment in settlement.0.active_apartments.iter_mut() {
                if let Ok(mut site) = construction_sites_q.get_mut(apartment.0) {
                    site.1.build_power_remaining -= 10;

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = server
                            .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ConstructionProgressChanged {
                                server_entity: site.0,
                                current_build_power: site.1.build_power_remaining,
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }

                    if site.1.build_power_remaining <= 0 {
                        commands.entity(apartment.0).despawn();

                        let new_apartment_tile = ((apartment.1.x / TILE_SIZE) as i32, (apartment.1.z / TILE_SIZE) as i32);

                        let new_apartment = commands.spawn(MaterialMeshBundle{
                            mesh: buildings_assets.apartment.0.clone(),
                            material: buildings_assets.apartment.1.clone(),
                            transform: Transform::from_translation(apartment.1).with_rotation(Quat::from_rotation_y(apartment.2)),
                            ..default()
                        })
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
                                new_apartment_tile,
                                (
                                    CompanyTypes::None,
                                    (0, 0, 0, 0, 0, 0, 0),
                                    "".to_string(),
                                ),
                            ),
                        })
                        .insert(CoverComponent{
                            cover_efficiency: 0.5,
                            points: vec![apartment.1, apartment.1, apartment.1, apartment.1, apartment.1, apartment.1, apartment.1, apartment.1, apartment.1, apartment.1],
                            units_inside: HashSet::new(),
                        })
                        .id();

                        if matches!(network_status.0, NetworkStatuses::Host) {
                            let mut channel_id = 30;
                            while channel_id <= 59 {
                                if let Err(_) = server
                                .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                    server_entity: apartment.0,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }

                            channel_id = 30;
                            while channel_id <= 59 {
                                if let Err(_) = server
                                .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ApartmentGenerated {
                                    team: team,
                                    server_entity: new_apartment,
                                    position: apartment.1,
                                    angle: apartment.2,
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        }

                        apartment.0 = new_apartment;

                        tile_map.tiles.entry(team).or_insert_with(HashMap::new).entry(new_apartment_tile)
                        .or_insert_with(HashMap::new).remove(&apartment.0);

                        tile_map.tiles.entry(team).or_insert_with(HashMap::new).entry(new_apartment_tile)
                        .or_insert_with(HashMap::new).insert(new_apartment, (apartment.1, UnitTypes::None));
                    }
                }
            }
        }
    }
}

#[derive(Component)]
pub struct SettlementCaptureInProgress;

#[derive(Component)]
pub struct SettlementCaptureProgressBar{
    pub constrcution_entity: Entity,
    pub max_width: f32,
}

pub fn capturing_displays_processing_system(
    mut progress_bars_q: Query<(&SettlementCaptureProgressBar, &mut Style, &Parent), With<SettlementCaptureProgressBar>>,
    mut progress_bar_holders_q: Query<(&mut Style, &Children), Without<SettlementCaptureProgressBar>>,
    settlements_q: Query<(&Transform, &SettlementComponent), With<SettlementCaptureInProgress>>,
    camera_q: Query<(&Camera, &GlobalTransform)>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
){
    let camera = camera_q.iter().next().unwrap();

    if camera.1.translation().y > 500. {
        for progress_bar in progress_bars_q.iter() {
            commands.entity(**progress_bar.2).insert(Visibility::Hidden);
        }

        return;
    }

    let bar_width = ui_button_nodes.button_size * 0.75;
    let bar_left_offset = bar_width / 2.;

    for mut progress_bar in progress_bars_q.iter_mut() {
        if let Ok(settlement) = settlements_q.get(progress_bar.0.constrcution_entity) {
            if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, settlement.0.translation) {
                if let Ok(mut holder) = progress_bar_holders_q.get_mut(**progress_bar.2) {
                    commands.entity(**progress_bar.2).insert(Visibility::Visible);

                    holder.0.left = Val::Px(viewport_point.x - bar_left_offset);
                    holder.0.top = Val::Px(viewport_point.y);

                    let fraction = settlement.1.0.elapsed_capture_time as f32 / settlement.1.0.time_to_capture as f32;

                    progress_bar.1.width = Val::Px(progress_bar.0.max_width * fraction);
                }
            }
        } else {
            commands.entity(**progress_bar.2).despawn_recursive();
        }
    }
}

#[derive(Resource)]
pub struct BuildingStageCache{
    pub buildings: HashMap<String, (i32, bool)>,
}

#[derive(Component)]
pub struct RoadBuilderComponent{
    pub last_position: Vec3,
    pub last_direction: Vec3,
    pub result_road_points: Vec<Vec3>,
}

pub fn roads_generation_system(
    mut event_reader: EventReader<AllApartmentsPlaced>,
    mut settlements_q: Query<(Entity, &Transform, &mut SettlementComponent), With<SettlementComponent>>,
    mut road_builders_q: Query<(Entity, &Transform, &mut RoadBuilderComponent, Option<&StoppedMoving>), Without<SettlementComponent>>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut event_writer: EventWriter<AllRoadsGenerated>,
    time: Res<Time>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for _event in event_reader.read() {
        let mut roads_to_generate: Vec<(Vec3, Vec3)> = Vec::new();
        let mut settlements_to_connect: Vec<(Entity, Entity)> = Vec::new();

        for settlement_main in settlements_q.iter() {
            for settlement_other in settlements_q.iter() {
                if settlement_main.1.translation.distance(settlement_other.1.translation) <= settlement_main.2.0.max_road_connection_distance &&
                settlement_main.0 != settlement_other.0 {
                    if !settlements_to_connect.contains(&(settlement_main.0, settlement_other.0)) && !settlements_to_connect.contains(&(settlement_other.0, settlement_main.0)) {
                        settlements_to_connect.push((settlement_main.0, settlement_other.0));

                        roads_to_generate.push((settlement_main.1.translation, settlement_other.1.translation));
                    }
                }
            }
        }

        for settlements in settlements_to_connect.iter() {
            if let Ok(mut settlement1) = settlements_q.get_mut(settlements.0) {
                settlement1.2.0.connected_settlements.push(settlements.1);
            }

            if let Ok(mut settlement2) = settlements_q.get_mut(settlements.1) {
                settlement2.2.0.connected_settlements.push(settlements.0);
            }
        }

        let mut settlements_clusters: Vec<Vec<Entity>> = Vec::new();
        let mut settlements_iter = settlements_q.iter();

        loop {
            if let Some(settlement) = settlements_iter.next() {
                let mut is_clustered = false;

                for cluster in settlements_clusters.iter() {
                    if cluster.contains(&settlement.0) {
                        is_clustered = true;
                        break;
                    }
                }

                if is_clustered {continue;}

                let mut new_cluster: Vec<Entity> = Vec::new();
                
                new_cluster.push(settlement.0);

                for connected_settlement in settlement.2.0.connected_settlements.iter() {
                    new_cluster.push(*connected_settlement);
                }

                let mut cluster_size = 0;
                loop {
                    let mut add_to_cluster: Vec<Entity> = Vec::new();
                    for entity in new_cluster.iter() {
                        if let Ok(clustered_settlement) = settlements_q.get(*entity) {
                            for connected_settlement in clustered_settlement.2.0.connected_settlements.iter() {
                                if !new_cluster.contains(connected_settlement) {
                                    add_to_cluster.push(*connected_settlement);
                                }
                            }
                        }
                    }

                    for to_add in add_to_cluster.iter() {
                        new_cluster.push(*to_add);
                    }

                    if cluster_size != new_cluster.len() {
                        cluster_size = new_cluster.len();
                    } else {
                        break;
                    }
                }

                settlements_clusters.push(new_cluster);
            } else {
                break;
            }
        }

        loop {
            if settlements_clusters.len() > 1 {
                let mut cluster_main = settlements_clusters[0].clone();

                let mut nearest_cluster = (f32::INFINITY, 0, Entity::PLACEHOLDER, Entity::PLACEHOLDER);
                for entity_main in cluster_main.iter() {
                    if let Ok(settlement_main) = settlements_q.get(*entity_main) {
                        for (index_other, cluster_other) in settlements_clusters.iter().enumerate() {
                            if index_other != 0 {
                                for entity_other in cluster_other.iter() {
                                    if let Ok(settlement_other) = settlements_q.get(*entity_other) {
                                        let distance = settlement_main.1.translation.distance(settlement_other.1.translation);
                                        if distance < nearest_cluster.0 {
                                            nearest_cluster = (distance, index_other, settlement_main.0, settlement_other.0);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut new_road = (Vec3::ZERO, Vec3::ZERO);
                if let Ok(mut settlement1) = settlements_q.get_mut(nearest_cluster.2) {
                    settlement1.2.0.connected_settlements.push(nearest_cluster.3);

                    new_road.0 = settlement1.1.translation;
                }

                if let Ok(mut settlement2) = settlements_q.get_mut(nearest_cluster.3) {
                    settlement2.2.0.connected_settlements.push(nearest_cluster.2);

                    new_road.1 = settlement2.1.translation;
                }

                roads_to_generate.push(new_road);

                let mut cluster_other = settlements_clusters[nearest_cluster.1].clone();

                settlements_clusters.remove(nearest_cluster.1);
                settlements_clusters.remove(0);

                cluster_main.append(&mut cluster_other);

                settlements_clusters.push(cluster_main);
            } else {
                break;
            }
        }

        for road in roads_to_generate.iter() {
            let mut road_path: Vec<Vec3> = Vec::new();

            if let Ok(nav_mesh) = nav_mesh.get().read() {
                match find_polygon_path(
                    &nav_mesh,
                    &nav_mesh_settings,
                    road.0,
                    road.1,
                    None,
                    Some(&[1.0, 1.0]),
                ) {
                    Ok(path) => {
                        match perform_string_pulling_on_path(&nav_mesh, road.0, road.1, &path) {
                            Ok(string_path) => {
                                road_path = string_path;
                            }
                            Err(error) => error!("Error with string path: {:?}", error),
                        };
                    }
                    Err(error) => error!("Error with pathfinding: {:?}", error),
                }
            }

            if road_path.len() > 1 {
                let spawn_pos = road.0 + Vec3::new(0., 0.5, 0.);
                commands.spawn((
                    MaterialMeshBundle{
                        mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(1., 500., 1.) }.mesh())),
                        material: materials.add(Color::srgba(1., 0., 1., 1.)),
                        transform: Transform::from_translation(spawn_pos),
                        ..default()
                    },
                    KinematicCharacterController{
                        custom_shape: Some((Collider::cuboid(0.25, 0.5, 0.25), Vec3::new(0., 0.5, 0.), Quat::IDENTITY)),
                        up: Vec3::Y,
                        offset: CharacterLength::Absolute(0.1),
                        slide: true,
                        autostep: None,
                        apply_impulse_to_dynamic_bodies: false,
                        snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                        filter_groups: Some(CollisionGroups::new(Group::all(), Group::GROUP_10)),
                        ..default()
                    },
                    RoadBuilderComponent{
                        last_position: spawn_pos,
                        last_direction: (road_path[0] - spawn_pos).normalize(),
                        result_road_points: vec![spawn_pos],
                    },
                    UnitComponent{
                        path: road_path,
                        start_position: Vec3::ZERO,
                        speed: 30.,
                        waypoint_radius: 0.5,
                        elapsed: 0.,
                        inv_duration: 0.,
                        last_position: Vec3::ZERO,
                        stuck_count: 0,
                    },
                    NeedToMove,
                ))
                .insert(Visibility::Hidden)
                .insert(NotShadowCaster);
            }
        }

        event_writer.send(AllRoadsGenerated);
    }

    for mut road_builder in road_builders_q.iter_mut() {
        let current_direction = ((road_builder.1.translation - road_builder.2.last_position).normalize() * 10.).round() / 10.;

        if road_builder.2.last_direction != current_direction {
            road_builder.2.last_direction = current_direction;

            let last_pos = road_builder.2.last_position;

            if road_builder.2.result_road_points[road_builder.2.result_road_points.len() - 1] != last_pos {
                road_builder.2.result_road_points.push(last_pos);
            }
        }

        road_builder.2.last_position = road_builder.1.translation;

        if let Some(_stopped) = road_builder.3 {
            if road_builder.2.result_road_points[road_builder.2.result_road_points.len() - 1] != road_builder.1.translation {
                road_builder.2.result_road_points.push(road_builder.1.translation);
            }

            let road_center = (road_builder.2.result_road_points[0] + road_builder.2.result_road_points[road_builder.2.result_road_points.len() - 1]) / 2.;

            let raod_mesh = create_curved_mesh(
                5.,
                5.,
                road_builder.2.result_road_points.clone(),
                -2.9,
                &Transform::from_translation(road_center),
            );

            let road_entity = commands.spawn(MaterialMeshBundle{
                mesh: meshes.add(raod_mesh.clone()),
                material: materials.add(Color::srgb(0.5, 0.5, 0.5)).into(),
                transform: Transform::from_translation(road_center),
                ..default()
            })
            .insert(Collider::from_bevy_mesh(&raod_mesh, &ComputedColliderShape::TriMesh).unwrap())
            .insert(NavMeshAffector)
            .insert(NavMeshAreaType(Some(Area(1))))
            .insert(NotShadowCaster)
            .insert(CollisionGroups::new(Group::GROUP_2, Group::all()))
            .id();

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 60;
                while channel_id <= 89 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::RoadGenerated {
                        road_points: road_builder.2.result_road_points.clone(),
                        road_center: road_center,
                        server_entity: road_entity,
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }

            commands.entity(road_builder.0).despawn();
        }
    }
}