use core::f32;
use std::f32::consts::E;

use bevy::{ecs::query, log, prelude::*, render::{mesh::{Indices, PrimitiveTopology}, render_asset::RenderAssetUsages}, tasks::AsyncComputeTaskPool, transform::commands, utils::hashbrown::HashMap};
use bevy_quinnet::server::QuinnetServer;
use bevy_rapier3d::{na::distance, rapier::{crossbeam::{channel, epoch::Pointable}, prelude::query_pipeline_generators::CurrentAabb}};
use oxidized_navigation_serializable::{NavMesh, NavMeshSettings};
use rand::distributions::DistMap;

use crate::{components::{asset_manager::UnitsAssets, building::SwitchableBuilding}, GameStage, GameStages, PlayerData};

use super::{building::{HumanResourceStorageComponent, MaterialsProductionComponent, MaterialsStorageComponent, SettlementComponent, SuppliesProductionComponent, SuppliesStorageComponent}, camera, network::{ClientList, NetworkStatus, NetworkStatuses, ServerMessage}, unit::{async_path_find, Armies, AsyncPathfindingTasks, AsyncTaskPools, CompanyTypes, CombatComponent, NeedToMove, SuppliesConsumerComponent, UnitComponent}};

// #[derive(Component)]
// pub struct RoadComponent(pub(
//     RoadObject,
//     Vec<Entity> //connected roads
// ));

// #[derive(Clone)]
// pub struct RoadObject {
//     pub road_points: Vec<Vec3>,
//     pub road_center: Vec3,
// }

pub const RESOURCE_ZONES_COUNT: i32 = 6;

#[derive(Component)]
pub struct ResourceZone{
    pub zone_radius: f32,
    pub current_miners: HashMap<i32, Option<(Entity, i32)>>,
}

#[derive(Component)]
pub struct LogisticUnitComponent {
    pub storage_capacity: i32,
    pub storage: ResourceTypes,
    pub destination: (Entity, Option<(CompanyTypes, (i32, i32, i32, i32, i32))>),
    pub last_destination_point: Vec3,
    pub path_recalculation_cooldown: u128,
    pub path_recalculation_elapsed: u128,
    pub destination_completion_range: f32,
}

pub enum ResourceTypes {
    Materials(i32),
    Supplies(i32),
    HumanResources(i32),
}

pub fn create_plane_between_points(
    object_transform: &Transform,
    start: Vec3,
    end: Vec3,
    width: f32,
) -> Mesh {
    let object_matrix = object_transform.compute_matrix();
    let inverse_matrix = object_matrix.inverse();

    let local_start = inverse_matrix.transform_point3(start);
    let local_end = inverse_matrix.transform_point3(end);

    let direction = (local_end - local_start).normalize();

    let up = Vec3::Y;

    let side = up.cross(direction).normalize() * (width / 2.0);

    let vertices = vec![
        local_start + side,
        local_start - side,
        local_end + side,
        local_end - side,
    ];

    let normals = vec![up; vertices.len()];

    let uvs = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [1.0, 1.0],
    ];

    let indices = Indices::U32(vec![
        0, 1, 2,
        2, 1, 3,
    ]);

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(indices);

    mesh
}

pub fn create_curved_mesh(
    width: f32,
    height: f32,
    path: Vec<Vec3>,
    vertical_offset: f32,
    entity_transform: &Transform,
) -> Mesh {
    let mut mesh = Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);

    let half_width = width / 2.0;
    let half_height = height / 2.0;

    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();

    let offset_path: Vec<Vec3> = path.iter().map(|point| *point + Vec3::Y * vertical_offset).collect();

    let local_path: Vec<Vec3> = offset_path
        .iter()
        .map(|point| entity_transform.compute_matrix().inverse() * point.extend(1.0))
        .map(|point| point.truncate())
        .collect();

    for i in 0..local_path.len() {
        let current_point = local_path[i];

        let tangent = if i == local_path.len() - 1 {
            local_path[i] - local_path[i - 1]
        } else if i == 0 {
            local_path[i + 1] - local_path[i]
        } else {
            local_path[i + 1] - local_path[i - 1]
        }
        .normalize_or_zero();

        let up = Vec3::Y;
        let normal = up.cross(tangent).normalize_or_zero();
        let binormal = normal.cross(tangent).normalize_or_zero();

        let v1 = current_point + normal * half_width + binormal * half_height;
        let v2 = current_point + normal * half_width - binormal * half_height;
        let v3 = current_point - normal * half_width - binormal * half_height;
        let v4 = current_point - normal * half_width + binormal * half_height;

        let base_index = vertices.len() as u32;
        vertices.extend_from_slice(&[v1, v2, v3, v4]);

        normals.extend_from_slice(&[
            normal, normal, normal, normal,
        ]);

        if i > 0 {
            indices.extend_from_slice(&[
                base_index - 4, base_index - 3, base_index,
                base_index, base_index - 3, base_index + 1,
            ]);

            indices.extend_from_slice(&[
                base_index - 2, base_index - 1, base_index + 2,
                base_index + 2, base_index - 1, base_index + 3,
            ]);

            indices.extend_from_slice(&[
                base_index - 4, base_index, base_index + 3,
                base_index + 3, base_index, base_index - 1,
            ]);

            indices.extend_from_slice(&[
                base_index - 3, base_index - 2, base_index + 2,
                base_index + 2, base_index + 1, base_index - 3,
            ]);
        }
    }

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

const LOGISTIC_UNITS_SPEED: f32 = 30.;

pub fn assign_supply_tasks (
    mut supply_participants: (
        Query<(&mut SuppliesProductionComponent, &Transform)>,
        Query<(Entity, &Transform, &mut SuppliesConsumerComponent), With<SuppliesConsumerComponent>>,
    ),
    army: Res<Armies>,
    mut commands: Commands,
    units_assets: Res<UnitsAssets>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    async_task_pools: Res<AsyncTaskPools>,
    game_stage: Res<GameStage>,
    player_data: Res<PlayerData>,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    // mut event_writer: (
    //     EventWriter<UnsentServerMessage>,
    // ),
){
    if !supply_participants.0.is_empty() && matches!(game_stage.0, GameStages::GameStarted) {
        for mut supply_producer in supply_participants.0.iter_mut() {
            supply_producer.0.elapsed_cooldown_time += time.delta().as_millis();
        }

        let mut supply_producers_iter = supply_participants.0.iter_mut();
        
        for regular_platoon in army.0.get(&player_data.team).unwrap().regular_platoons.iter() {
            if !commands.get_entity(regular_platoon.1.2).is_none() {
                if let Ok (mut supply_consumer) = supply_participants.1.get_mut(regular_platoon.1.2) {
                    supply_consumer.2.elapsed_time += time.delta().as_millis();

                    if supply_producers_iter.len() == 0 {continue;}
    
                    if supply_consumer.2.elapsed_time >= supply_consumer.2.supply_frequency {
                        while let Some(mut supply_producer) = supply_producers_iter.next() {
                            let supplies_needed = (regular_platoon.1.0.0.0.capacity() + regular_platoon.1.0.0.1.capacity()) as i32 * supply_consumer.2.supplies_capacity;

                            if supply_producer.0.elapsed_cooldown_time >= supply_producer.0.supply_cooldown && supply_producer.0.available_supplies >= supplies_needed {
                                supply_producer.0.elapsed_cooldown_time = 0;
                                supply_consumer.2.elapsed_time = 0;
                                
                                supply_producer.0.available_supplies -= supplies_needed;

                                let start_point = supply_producer.1.translation + supply_producer.0.production_local_point;
    
                                let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                    mesh: units_assets.truck.0.clone(),
                                    material: units_assets.truck.1.clone(),
                                    transform: Transform::from_translation(start_point),
                                    ..default()
                                }).insert(UnitComponent {
                                    path: Vec::new(),
                                    speed: LOGISTIC_UNITS_SPEED,
                                }).insert(LogisticUnitComponent {
                                    storage_capacity: supplies_needed,
                                    storage: ResourceTypes::Supplies(supplies_needed),
                                    destination: (supply_consumer.0, Some((CompanyTypes::Regular, *regular_platoon.0))),
                                    last_destination_point: supply_consumer.1.translation,
                                    path_recalculation_cooldown: 5000,
                                    path_recalculation_elapsed: 0,
                                    destination_completion_range: supply_consumer.2.supply_range,
                                }).id();
        
                                let nav_mesh_lock = nav_mesh.get();
                
                                let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                    nav_mesh_lock.clone(),
                                    nav_mesh_settings.clone(),
                                    start_point,
                                    supply_consumer.1.translation,
                                    Some(100.),
                                    Some(&[10.0, 0.1]),
                                    new_logistic_unit,
                                ));
                
                                pathfinding_task.tasks.push(task);

                                if matches!(network_status.0, NetworkStatuses::Host){
                                    let mut channel_id = 60;
                                    while channel_id <= 89 {
                                        if let Err(_) = server
                                        .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::LogisticUnitSpawned {
                                            position: Vec3::new(supply_producer.1.translation.x, 0.25, supply_producer.1.translation.z),
                                            server_entity: new_logistic_unit,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
    
                                break;
                            }
                        }
                    } else {
                        continue;
                    }
                }
            }
        }

        if supply_producers_iter.len() > 0 {
            for shock_platoon in army.0.get(&player_data.team).unwrap().shock_platoons.iter() {
                if !commands.get_entity(shock_platoon.1.2).is_none() {
                    if let Ok (mut supply_consumer) = supply_participants.1.get_mut(shock_platoon.1.2) {
                        supply_consumer.2.elapsed_time += time.delta().as_millis();

                        if supply_producers_iter.len() == 0 {continue;}
        
                        if supply_consumer.2.elapsed_time >= supply_consumer.2.supply_frequency {
                            while let Some(mut supply_producer) = supply_producers_iter.next() {
                                let supplies_needed = (shock_platoon.1.0.0.0.capacity() + shock_platoon.1.0.0.1.capacity()) as i32 * supply_consumer.2.supplies_capacity;

                                if supply_producer.0.elapsed_cooldown_time >= supply_producer.0.supply_cooldown && supply_producer.0.available_supplies >= supplies_needed {
                                    supply_producer.0.elapsed_cooldown_time = 0;
                                    supply_consumer.2.elapsed_time = 0;

                                    supply_producer.0.available_supplies -= supplies_needed;

                                    let start_point = supply_producer.1.translation + supply_producer.0.production_local_point;
        
                                    let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                        mesh: units_assets.truck.0.clone(),
                                        material: units_assets.truck.1.clone(),
                                        transform: Transform::from_translation(start_point),
                                        ..default()
                                    }).insert(UnitComponent {
                                        path: Vec::new(),
                                        speed: LOGISTIC_UNITS_SPEED,
                                    }).insert(LogisticUnitComponent {
                                        storage_capacity: supplies_needed,
                                        storage: ResourceTypes::Supplies(supplies_needed),
                                        destination: (supply_consumer.0, Some((CompanyTypes::Shock, *shock_platoon.0))),
                                        last_destination_point: supply_consumer.1.translation,
                                        path_recalculation_cooldown: 5000,
                                        path_recalculation_elapsed: 0,
                                        destination_completion_range: supply_consumer.2.supply_range,
                                    }).id();
            
                                    let nav_mesh_lock = nav_mesh.get();
                    
                                    let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                        nav_mesh_lock.clone(),
                                        nav_mesh_settings.clone(),
                                        start_point,
                                        supply_consumer.1.translation,
                                        Some(100.),
                                        Some(&[10.0, 0.1]),
                                        new_logistic_unit,
                                    ));
                    
                                    pathfinding_task.tasks.push(task);

                                    if matches!(network_status.0, NetworkStatuses::Host){
                                        let mut channel_id = 60;
                                        while channel_id <= 89 {
                                            if let Err(_) = server.endpoint_mut()
                                            .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::LogisticUnitSpawned {
                                                position: Vec3::new(supply_producer.1.translation.x, 0.25, supply_producer.1.translation.z),
                                                server_entity: new_logistic_unit,
                                            }){
                                                channel_id += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
        
                                    break;
                                }
                            }
                        } else {
                            continue;
                        }
                    }
                }
            }

            if supply_producers_iter.len() > 0 {
                for armored_platoon in army.0.get(&player_data.team).unwrap().armored_platoons.iter() {
                    if !commands.get_entity(armored_platoon.1.2).is_none() {
                        if let Ok (mut supply_consumer) = supply_participants.1.get_mut(armored_platoon.1.2) {
                            supply_consumer.2.elapsed_time += time.delta().as_millis();

                            if supply_producers_iter.len() == 0 {continue;}
            
                            if supply_consumer.2.elapsed_time >= supply_consumer.2.supply_frequency {
                                while let Some(mut supply_producer) = supply_producers_iter.next() {
                                    let supplies_needed = armored_platoon.1.0.0.capacity() as i32 * supply_consumer.2.supplies_capacity;

                                    if supply_producer.0.elapsed_cooldown_time >= supply_producer.0.supply_cooldown && supply_producer.0.available_supplies >= supplies_needed {
                                        supply_producer.0.elapsed_cooldown_time = 0;
                                        supply_consumer.2.elapsed_time = 0;

                                        supply_producer.0.available_supplies -= supplies_needed;

                                        let start_point = supply_producer.1.translation + supply_producer.0.production_local_point;
            
                                        let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                            mesh: units_assets.truck.0.clone(),
                                            material: units_assets.truck.1.clone(),
                                            transform: Transform::from_translation(start_point),
                                            ..default()
                                        }).insert(UnitComponent {
                                            path: Vec::new(),
                                            speed: LOGISTIC_UNITS_SPEED,
                                        }).insert(LogisticUnitComponent {
                                            storage_capacity: supplies_needed,
                                            storage: ResourceTypes::Supplies(supplies_needed),
                                            destination: (supply_consumer.0, Some((CompanyTypes::Armored, *armored_platoon.0))),
                                            last_destination_point: supply_consumer.1.translation,
                                            path_recalculation_cooldown: 5000,
                                            path_recalculation_elapsed: 0,
                                            destination_completion_range: supply_consumer.2.supply_range,
                                        }).id();
                
                                        let nav_mesh_lock = nav_mesh.get();
                        
                                        let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                            nav_mesh_lock.clone(),
                                            nav_mesh_settings.clone(),
                                            start_point,
                                            supply_consumer.1.translation,
                                            Some(100.),
                                            Some(&[10.0, 0.1]),
                                            new_logistic_unit,
                                        ));
                        
                                        pathfinding_task.tasks.push(task);

                                        if matches!(network_status.0, NetworkStatuses::Host){
                                            let mut channel_id = 60;
                                            while channel_id <= 89 {
                                                if let Err(_) = server
                                                .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::LogisticUnitSpawned {
                                                    position: Vec3::new(supply_producer.1.translation.x, 0.25, supply_producer.1.translation.z),
                                                    server_entity: new_logistic_unit,
                                                }){
                                                    channel_id += 1;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
            
                                        break;
                                    }
                                }
                            } else {
                                continue;
                            }
                        }
                    }
                }

                if supply_producers_iter.len() > 0 {
                    for artillery_unit in army.0.get(&player_data.team).unwrap().artillery_units.0.iter() {
                        if let Some (unit_entity) = artillery_unit.1.0.0 {
                            if !commands.get_entity(unit_entity).is_none() {
                                if let Ok (mut supply_consumer) = supply_participants.1.get_mut(unit_entity) {
                                    supply_consumer.2.elapsed_time += time.delta().as_millis();

                                    if supply_producers_iter.len() == 0 {continue;}
                    
                                    if supply_consumer.2.elapsed_time >= supply_consumer.2.supply_frequency {
                                        while let Some(mut supply_producer) = supply_producers_iter.next() {
                                            let supplies_needed = supply_consumer.2.supplies_capacity;

                                            if supply_producer.0.elapsed_cooldown_time >= supply_producer.0.supply_cooldown &&
                                            supply_producer.0.available_supplies >= supplies_needed {
                                                supply_producer.0.elapsed_cooldown_time = 0;
                                                supply_consumer.2.elapsed_time = 0;

                                                supply_producer.0.available_supplies -= supplies_needed;

                                                let start_point = supply_producer.1.translation + supply_producer.0.production_local_point;

                                                let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                                    mesh: units_assets.truck.0.clone(),
                                                    material: units_assets.truck.1.clone(),
                                                    transform: Transform::from_translation(start_point),
                                                    ..default()
                                                }).insert(UnitComponent {
                                                    path: Vec::new(),
                                                    speed: LOGISTIC_UNITS_SPEED,
                                                }).insert(LogisticUnitComponent {
                                                    storage_capacity: supplies_needed,
                                                    storage: ResourceTypes::Supplies(supplies_needed),
                                                    destination: (supply_consumer.0, None),
                                                    last_destination_point: supply_consumer.1.translation,
                                                    path_recalculation_cooldown: 5000,
                                                    path_recalculation_elapsed: 0,
                                                    destination_completion_range: supply_consumer.2.supply_range,
                                                }).id();
                        
                                                let nav_mesh_lock = nav_mesh.get();
                                
                                                let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                                    nav_mesh_lock.clone(),
                                                    nav_mesh_settings.clone(),
                                                    start_point,
                                                    supply_consumer.1.translation,
                                                    Some(100.),
                                                    Some(&[10.0, 0.1]),
                                                    new_logistic_unit,
                                                ));
                                
                                                pathfinding_task.tasks.push(task);

                                                if matches!(network_status.0, NetworkStatuses::Host){
                                                    let mut channel_id = 60;
                                                    while channel_id <= 89 {
                                                        if let Err(_) = server.endpoint_mut()
                                                        .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::LogisticUnitSpawned {
                                                            position: Vec3::new(supply_producer.1.translation.x, 0.25, supply_producer.1.translation.z),
                                                            server_entity: new_logistic_unit,
                                                        }){
                                                            channel_id += 1;
                                                        } else {
                                                            break;
                                                        }
                                                    }
                                                }
                    
                                                break;
                                            }
                                        }
                                    } else {
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn logistic_convoys_processing_system(
    mut logistic_units_q: Query<(Entity, &Transform, &mut LogisticUnitComponent, &mut UnitComponent), (With<LogisticUnitComponent>, Without<SuppliesConsumerComponent>)>,
    mut supply_consumers_q: Query<(&Transform, &mut SuppliesConsumerComponent, &UnitComponent), (With<SuppliesConsumerComponent>, Without<LogisticUnitComponent>)>,
    mut material_consumers_q: Query<&mut MaterialsStorageComponent, With<MaterialsStorageComponent>>,
    mut human_resource_consumers_q: Query<&mut HumanResourceStorageComponent, With<HumanResourceStorageComponent>>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    async_task_pools: Res<AsyncTaskPools>,
    army: Res<Armies>,
    mut commands: Commands,
    player_data: Res<PlayerData>,
    timer: ResMut<camera::TimerResource>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    // mut event_writer: (
    //     EventWriter<UnsentServerMessage>,
    // ),
){
    if timer.0.finished() {
        for mut logistic_unit in logistic_units_q.iter_mut() {
            match logistic_unit.2.storage {
                ResourceTypes::Materials(count) => {
                    if logistic_unit.1.translation.distance(logistic_unit.2.last_destination_point) <= logistic_unit.2.destination_completion_range {
                        if let Ok(mut material_consumer) = material_consumers_q.get_mut(logistic_unit.2.destination.0){
                            material_consumer.available_resources += count;

                            if material_consumer.available_resources > material_consumer.materials_storage_capacity {
                                material_consumer.available_resources = material_consumer.materials_storage_capacity;
                            }

                            commands.entity(logistic_unit.0).despawn();

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                        server_entity: logistic_unit.0,
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
                ResourceTypes::Supplies(count) => {
                    if let Ok(mut supply_consumer) = supply_consumers_q.get_mut(logistic_unit.2.destination.0) {
                        if supply_consumer.2.path.is_empty() {
                            if logistic_unit.2.last_destination_point.distance(supply_consumer.0.translation) > supply_consumer.1.supply_range {
                                logistic_unit.2.path_recalculation_elapsed += 500;
    
                                if logistic_unit.2.path_recalculation_elapsed >= logistic_unit.2.path_recalculation_cooldown {
                                    logistic_unit.2.path_recalculation_elapsed = 0;
    
                                    let nav_mesh_lock = nav_mesh.get();

                                    logistic_unit.3.path = Vec::new();
                
                                    let task = async_task_pools.extra_pathfinding_pool.spawn(async_path_find(
                                        nav_mesh_lock.clone(),
                                        nav_mesh_settings.clone(),
                                        logistic_unit.1.translation,
                                        supply_consumer.0.translation,
                                        Some(100.),
                                        Some(&[10.0, 0.1]),
                                        logistic_unit.0,
                                    ));
                    
                                    pathfinding_task.tasks.push(task);
        
                                    logistic_unit.2.last_destination_point = supply_consumer.0.translation;
                                }
                            }
                        } else {
                            if logistic_unit.2.last_destination_point.distance(supply_consumer.2.path[supply_consumer.2.path.len() - 1]) > supply_consumer.1.supply_range {
                                logistic_unit.2.path_recalculation_elapsed += 500;
    
                                if logistic_unit.2.path_recalculation_elapsed >= logistic_unit.2.path_recalculation_cooldown {
                                    logistic_unit.2.path_recalculation_elapsed = 0;
    
                                    let nav_mesh_lock = nav_mesh.get();

                                    logistic_unit.3.path = Vec::new();
                
                                    let task = async_task_pools.extra_pathfinding_pool.spawn(async_path_find(
                                        nav_mesh_lock.clone(),
                                        nav_mesh_settings.clone(),
                                        logistic_unit.1.translation,
                                        supply_consumer.2.path[supply_consumer.2.path.len() - 1],
                                        Some(100.),
                                        Some(&[10.0, 0.1]),
                                        logistic_unit.0,
                                    ));
                    
                                    pathfinding_task.tasks.push(task);
        
                                    logistic_unit.2.last_destination_point = supply_consumer.2.path[supply_consumer.2.path.len() - 1];
                                }
                            }
                        }
        
                        if logistic_unit.1.translation.distance(supply_consumer.0.translation) <= supply_consumer.1.supply_range {
                            let mut supplies = count;

                            if let Some(platoon) = logistic_unit.2.destination.1 {
                                match platoon.0 {
                                    CompanyTypes::Regular => {
                                        if let Some(regular_platoon) = army.0.get(&player_data.team).unwrap().regular_platoons.get(&platoon.1) {
                                            for unit in regular_platoon.0.0.0.set.iter() {
                                                if let Ok(mut consumer) = supply_consumers_q.get_mut(*unit) {
                                                    if supplies >= consumer.1.supplies_capacity {
                                                        supplies -= consumer.1.supplies_capacity;
                                                        consumer.1.supplies = consumer.1.supplies_capacity;
                                                    } else {
                                                        consumer.1.supplies += supplies;
                                                        supplies = 0;

                                                        break;
                                                    }
                                                }
                                            }

                                            for unit in regular_platoon.0.0.1.set.iter() {
                                                if let Ok(mut consumer) = supply_consumers_q.get_mut(*unit) {
                                                    if supplies >= consumer.1.supplies_capacity {
                                                        supplies -= consumer.1.supplies_capacity;
                                                        consumer.1.supplies = consumer.1.supplies_capacity;
                                                    } else {
                                                        consumer.1.supplies += supplies;
                                                        supplies = 0;

                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    CompanyTypes::Shock => {
                                        if let Some(shock_platoon) = army.0.get(&player_data.team).unwrap().shock_platoons.get(&platoon.1) {
                                            for unit in shock_platoon.0.0.0.set.iter() {
                                                if let Ok(mut consumer) = supply_consumers_q.get_mut(*unit) {
                                                    if supplies >= consumer.1.supplies_capacity {
                                                        supplies -= consumer.1.supplies_capacity;
                                                        consumer.1.supplies = consumer.1.supplies_capacity;
                                                    } else {
                                                        consumer.1.supplies += supplies;
                                                        supplies = 0;

                                                        break;
                                                    }
                                                }
                                            }

                                            for unit in shock_platoon.0.0.1.set.iter() {
                                                if let Ok(mut consumer) = supply_consumers_q.get_mut(*unit) {
                                                    if supplies >= consumer.1.supplies_capacity {
                                                        supplies -= consumer.1.supplies_capacity;
                                                        consumer.1.supplies = consumer.1.supplies_capacity;
                                                    } else {
                                                        consumer.1.supplies += supplies;
                                                        supplies = 0;

                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    CompanyTypes::Armored => {
                                        if let Some(armored_platoon) = army.0.get(&player_data.team).unwrap().armored_platoons.get(&platoon.1) {
                                            for unit in armored_platoon.0.0.set.iter() {
                                                if let Ok(mut consumer) = supply_consumers_q.get_mut(*unit) {
                                                    if supplies >= consumer.1.supplies_capacity {
                                                        supplies -= consumer.1.supplies_capacity;
                                                        consumer.1.supplies = consumer.1.supplies_capacity;
                                                    } else {
                                                        consumer.1.supplies += supplies;
                                                        supplies = 0;

                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    _ => {},
                                }
                            } else {
                                supply_consumer.1.supplies += supplies;
                                if supply_consumer.1.supplies > supply_consumer.1.supplies_capacity {
                                    supply_consumer.1.supplies = supply_consumer.1.supplies_capacity;
                                }
                            }

                            commands.entity(logistic_unit.0).despawn();

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                        server_entity: logistic_unit.0,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    else {
                        if let Some(platoon) = logistic_unit.2.destination.1 {
                            let mut platoon_leader_entity = Entity::PLACEHOLDER;
        
                            match platoon.0 {
                                CompanyTypes::Regular => {
                                    if let Some(regular_platoon) = army.0.get(&player_data.team).unwrap().regular_platoons.get(&platoon.1) {
                                        if commands.get_entity(regular_platoon.2).is_some() {
                                            platoon_leader_entity = regular_platoon.2;
                                        }
                                    }
                                },
                                CompanyTypes::Shock => {
                                    if let Some(shock_platoon) = army.0.get(&player_data.team).unwrap().shock_platoons.get(&platoon.1) {
                                        if commands.get_entity(shock_platoon.2).is_some() {
                                            platoon_leader_entity = shock_platoon.2;
                                        }
                                    }
                                },
                                CompanyTypes::Armored => {
                                    if let Some(armored_platoon) = army.0.get(&player_data.team).unwrap().armored_platoons.get(&platoon.1) {
                                        if commands.get_entity(armored_platoon.2).is_some() {
                                            platoon_leader_entity = armored_platoon.2;
                                        }
                                    }
                                },
                                _ => {},
                            }
        
                            if platoon_leader_entity != Entity::PLACEHOLDER {
                                logistic_unit.2.destination.0 = platoon_leader_entity;
                            }
                            else{
                                commands.entity(logistic_unit.0).despawn();

                                if matches!(network_status.0, NetworkStatuses::Host){
                                    let mut channel_id = 60;
                                    while channel_id <= 89 {
                                        if let Err(_) = server.endpoint_mut()
                                        .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                            server_entity: logistic_unit.0,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            }
                        } else {
                            commands.entity(logistic_unit.0).despawn();

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server
                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                        server_entity: logistic_unit.0,
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
                ResourceTypes::HumanResources(count) => {
                    if logistic_unit.1.translation.distance(logistic_unit.2.last_destination_point) <= logistic_unit.2.destination_completion_range {
                        if let Ok(mut human_resource_consumer) = human_resource_consumers_q.get_mut(logistic_unit.2.destination.0){
                            human_resource_consumer.available_human_resources += count;

                            if human_resource_consumer.available_human_resources > human_resource_consumer.human_resource_storage_capacity {
                                human_resource_consumer.available_human_resources = human_resource_consumer.human_resource_storage_capacity;
                            }

                            commands.entity(logistic_unit.0).despawn();

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                        server_entity: logistic_unit.0,
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
            }
        }
    }
}

pub fn material_producers_processing_system(
    mut material_producers_q: Query<(&Transform, &mut MaterialsProductionComponent), With<MaterialsProductionComponent>>,
    mut material_consumers_q: Query<(Entity, &Transform, &mut MaterialsStorageComponent), With<MaterialsStorageComponent>>,
    units_assets: Res<UnitsAssets>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    async_task_pools: Res<AsyncTaskPools>,
    mut commands: Commands,
    game_stage: Res<GameStage>,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    // mut event_writer: (
    //     EventWriter<UnsentServerMessage>,
    // ),
){
    if matches!(game_stage.0, GameStages::GameStarted) {
        for mut material_consumer in material_consumers_q.iter_mut() {
            material_consumer.2.replenishment_time_elapsed += time.delta().as_millis();
        }
        
        let mut material_consumers_iter = material_consumers_q.iter_mut();
        let mut is_consumers_empty = false;

        for mut material_producer in material_producers_q.iter_mut(){
            material_producer.1.elapsed_time += time.delta().as_millis();
    
            if material_producer.1.elapsed_time >= material_producer.1.materials_production_speed &&
            material_producer.1.available_materials < material_producer.1.materials_storage_capacity {
                material_producer.1.elapsed_time = 0;
    
                material_producer.1.available_materials += material_producer.1.materials_production_rate;
    
                if material_producer.1.available_materials > material_producer.1.materials_storage_capacity {
                    material_producer.1.available_materials = material_producer.1.materials_storage_capacity;
                }
            }

            loop {
                if let Some(mut material_consumer) = material_consumers_iter.next() {
                    if material_consumer.2.replenishment_time_elapsed >= material_consumer.2.replenishment_cooldown {
                        material_consumer.2.replenishment_time_elapsed = 0;

                        if
                        material_producer.1.available_materials >= material_consumer.2.replenishment_amount &&
                        material_consumer.2.available_resources < material_consumer.2.materials_storage_capacity {
                            material_producer.1.available_materials -= material_consumer.2.replenishment_amount;

                            let start_point = material_producer.0.translation + material_producer.1.production_local_point;
                            let destination_point = material_consumer.1.translation + material_consumer.2.replenishment_local_point;

                            let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                mesh: units_assets.truck.0.clone(),
                                material: units_assets.truck.1.clone(),
                                transform: Transform::from_translation(start_point),
                                ..default()
                            }).insert(UnitComponent {
                                path: Vec::new(),
                                speed: LOGISTIC_UNITS_SPEED,
                            }).insert(LogisticUnitComponent {
                                storage_capacity: material_consumer.2.replenishment_amount,
                                storage: ResourceTypes::Materials(material_consumer.2.replenishment_amount),
                                destination: (material_consumer.0, None),
                                last_destination_point: destination_point,
                                path_recalculation_cooldown: 5000,
                                path_recalculation_elapsed: 0,
                                destination_completion_range: material_consumer.2.replenishment_range,
                            }).id();

                            let nav_mesh_lock = nav_mesh.get();
            
                            let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                nav_mesh_lock.clone(),
                                nav_mesh_settings.clone(),
                                start_point,
                                destination_point,
                                Some(100.),
                                Some(&[10.0, 0.1]),
                                new_logistic_unit,
                            ));
            
                            pathfinding_task.tasks.push(task);

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut()
                                    .send_group_message_on(clients.0.keys(), 1, ServerMessage::LogisticUnitSpawned {
                                        position: Vec3::new(material_producer.0.translation.x, 0.25, material_producer.0.translation.z),
                                        server_entity: new_logistic_unit,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }

                            break;
                        }
                    }
                } else {
                    is_consumers_empty = true;
                    break;
                }
            }

            // if is_consumers_empty {
            //     break;
            // }
        }
    }
}

pub fn human_resource_producers_processing_system(
    mut human_resource_producers_q: Query<(&Transform, &mut SettlementComponent), With<SettlementComponent>>,
    mut human_resource_consumers_q: Query<(Entity, &Transform, &mut HumanResourceStorageComponent), With<HumanResourceStorageComponent>>,
    units_assets: Res<UnitsAssets>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    async_task_pools: Res<AsyncTaskPools>,
    mut commands: Commands,
    game_stage: Res<GameStage>,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    if matches!(game_stage.0, GameStages::GameStarted) {
        for mut human_resource_consumer in human_resource_consumers_q.iter_mut() {
            human_resource_consumer.2.replenishment_time_elapsed += time.delta().as_millis();
        }

        let mut human_resource_consumers_iter = human_resource_consumers_q.iter_mut();
        let mut is_consumers_empty = false;
        
        for mut human_resource_producer in human_resource_producers_q.iter_mut(){
            human_resource_producer.1.0.elapsed_time += time.delta().as_millis();
    
            if human_resource_producer.1.0.elapsed_time >= human_resource_producer.1.0.human_resource_production_speed &&
            human_resource_producer.1.0.available_human_resources < human_resource_producer.1.0.human_resource_storage_capacity {
                human_resource_producer.1.0.elapsed_time = 0;
    
                human_resource_producer.1.0.available_human_resources += human_resource_producer.1.0.human_resource_production_rate;
    
                if human_resource_producer.1.0.available_human_resources > human_resource_producer.1.0.human_resource_storage_capacity {
                    human_resource_producer.1.0.available_human_resources = human_resource_producer.1.0.human_resource_storage_capacity;
                }
            }

            loop {
                if let Some(mut human_resource_consumer) = human_resource_consumers_iter.next() {
                    if human_resource_consumer.2.replenishment_time_elapsed >= human_resource_consumer.2.replenishment_cooldown {
                        human_resource_consumer.2.replenishment_time_elapsed = 0;

                        if
                        human_resource_producer.1.0.available_human_resources >= human_resource_consumer.2.replenishment_amount &&
                        human_resource_consumer.2.available_human_resources < human_resource_consumer.2.human_resource_storage_capacity {
                            human_resource_producer.1.0.available_human_resources -= human_resource_consumer.2.replenishment_amount;

                            let start_point = human_resource_producer.0.translation + human_resource_producer.1.0.production_local_point;
                            let destination_point = human_resource_consumer.1.translation + human_resource_consumer.2.replenishment_local_point;

                            let new_logistic_unit = commands.spawn(MaterialMeshBundle {
                                mesh: units_assets.truck.0.clone(),
                                material: units_assets.truck.1.clone(),
                                transform: Transform::from_translation(start_point),
                                ..default()
                            }).insert(UnitComponent {
                                path: Vec::new(),
                                speed: LOGISTIC_UNITS_SPEED,
                            }).insert(LogisticUnitComponent {
                                storage_capacity: human_resource_consumer.2.replenishment_amount,
                                storage: ResourceTypes::HumanResources(human_resource_consumer.2.replenishment_amount),
                                destination: (human_resource_consumer.0, None),
                                last_destination_point: destination_point,
                                path_recalculation_cooldown: 5000,
                                path_recalculation_elapsed: 0,
                                destination_completion_range: human_resource_consumer.2.replenishment_range,
                            }).id();

                            let nav_mesh_lock = nav_mesh.get();
            
                            let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
                                nav_mesh_lock.clone(),
                                nav_mesh_settings.clone(),
                                start_point,
                                destination_point,
                                Some(100.),
                                Some(&[10.0, 0.1]),
                                new_logistic_unit,
                            ));
            
                            pathfinding_task.tasks.push(task);

                            if matches!(network_status.0, NetworkStatuses::Host){
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server
                                    .endpoint_mut().send_group_message_on(clients.0.keys(), 1, ServerMessage::LogisticUnitSpawned {
                                        position: Vec3::new(human_resource_producer.0.translation.x, 0.25, human_resource_producer.0.translation.z),
                                        server_entity: new_logistic_unit,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }

                            break;
                        }
                    }
                } else {
                    is_consumers_empty = true;
                    break;
                }
            }

            // if is_consumers_empty {
            //     break;
            // }
        }
    }
}

pub fn supplies_production_system (
    mut supplies_producers_q: Query<(&mut SuppliesProductionComponent, &mut MaterialsStorageComponent, &SwitchableBuilding)>,
    time: Res<Time>,
    game_stage: Res<GameStage>,
){
    if matches!(game_stage.0, GameStages::GameStarted) {
        for mut supply_producer in supplies_producers_q.iter_mut() {
            if !supply_producer.2.0 {continue;}
            
            supply_producer.0.elapsed_production_time += time.delta().as_millis();

            if supply_producer.0.elapsed_production_time >= supply_producer.0.supplies_production.1.time_to_produce {
                supply_producer.0.elapsed_production_time = 0;

                if supply_producer.0.available_supplies == supply_producer.0.supplies_storage_capacity {continue;}

                if supply_producer.1.available_resources >= supply_producer.0.supplies_production.1.resource_cost {
                    supply_producer.1.available_resources -= supply_producer.0.supplies_production.1.resource_cost;
                    
                    supply_producer.0.available_supplies += supply_producer.0.supplies_production.0;

                    if supply_producer.0.available_supplies > supply_producer.0.supplies_storage_capacity {
                        let surplus_supplies = supply_producer.0.available_supplies - supply_producer.0.supplies_storage_capacity;
                        let actually_produced = supply_producer.0.supplies_production.0 - surplus_supplies;
                        let production_excess_fraction = 1. - actually_produced as f32 / supply_producer.0.supplies_production.0 as f32;

                        supply_producer.0.available_supplies = supply_producer.0.supplies_storage_capacity;
                        supply_producer.1.available_resources += (supply_producer.0.supplies_production.1.resource_cost as f32 * production_excess_fraction) as i32;

                        if supply_producer.1.available_resources > supply_producer.1.materials_storage_capacity {
                            supply_producer.1.available_resources = supply_producer.1.materials_storage_capacity;
                        }
                    }
                }
            }
        }
    }
}

// pub fn logistic_tasks_processing_system(// Полюбому оптимизировать жестко надо
//     mut logistic_units: Query<(&mut UnitComponent, &mut LogisticUnitComponent, &mut Transform, Entity)>,
//     roads: Query<(Entity, &RoadComponent)>,
//     supply_consumers: Query<(&SuppliesConsumerComponent, &Transform), Without<LogisticUnitComponent>>,
//     army: Res<Army>,
//     nav_mesh: Res<NavMesh>,
//     nav_mesh_settings: Res<NavMeshSettings>,
//     mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
//     time: Res<Time>,
//     mut commands: Commands,
// ){
//     for mut logistic_unit in logistic_units.iter_mut() {
//         let mut is_moved = false;

//         let mut speed = logistic_unit.0.speed;
//         if logistic_unit.1.destination[0].2 {
//             speed *= 2.;
//         }

//         if logistic_unit.0.path == vec![] && logistic_unit.1.destination[0].0 != Entity::PLACEHOLDER {
//             if let Ok (road) = roads.get(logistic_unit.1.destination[0].0){
//                 match logistic_unit.1.destination[0].1.0 {
//                     UnitTypes::Regular => {
//                         if let Some (platoon) = army.regular_platoons.get(&logistic_unit.1.destination[0].1.1) {
//                             if let Ok (consumer) = supply_consumers.get(platoon.2) {
//                                 let nearest_road_end;
//                                 let farthest_road_end;
//                                 if road.1.0.0.road_points[0].distance(consumer.1.translation) < road.1.0.0.road_points[1].distance(consumer.1.translation) {
//                                     nearest_road_end = road.1.0.0.road_points[0];
//                                     farthest_road_end = road.1.0.0.road_points[1];
//                                 } else {
//                                     nearest_road_end = road.1.0.0.road_points[1];
//                                     farthest_road_end = road.1.0.0.road_points[0];
//                                 }

//                                 if nearest_road_end.distance(logistic_unit.2.translation) > 1. {
//                                     logistic_unit.0.path = vec![nearest_road_end];

//                                     if !logistic_unit.1.destination[0].2 {
//                                         let current_road_distance = road.1.0.0.road_center.distance(consumer.1.translation);
//                                         let mut counter = road.1.0.1.len();
//                                         for road_entity in road.1.0.1.iter() {
//                                             if let Ok(another_road) = roads.get(*road_entity) {
//                                                 let another_road_distance = another_road.1.0.0.road_center.distance(consumer.1.translation);
//                                                 if current_road_distance < another_road_distance {
//                                                     counter -= 1;
//                                                 }
//                                             }
//                                         }
    
//                                         if counter == 0 {
//                                             logistic_unit.1.is_on_final_road = true;
//                                         }
//                                     }

//                                     logistic_unit.1.destination[0].2 = true;
//                                 } else {
//                                     logistic_unit.0.path = vec![farthest_road_end];
//                                     logistic_unit.1.destination[0].2 = true;
//                                     logistic_unit.1.is_on_final_road = true;
//                                 }
//                             }
//                         }
//                     },
//                     UnitTypes::Shock => {

//                     },
//                     UnitTypes::Armored => {

//                     },
//                     UnitTypes::None => {},
//                 }
//             }
//         }
//         else if logistic_unit.0.path != vec![] && logistic_unit.1.destination[0].2 {
//             let next_point = logistic_unit.0.path[0];
//             let mut direction = (next_point - logistic_unit.2.translation).normalize();
//             direction.y = 0.0;
//             logistic_unit.2.translation += direction * speed * time.delta_seconds();
//             is_moved = true;

//             if logistic_unit.2.translation.xz().distance(logistic_unit.0.path[0].xz()) < 0.1 {
//                 logistic_unit.0.path.remove(0);

//                 if let Ok (road) = roads.get(logistic_unit.1.destination[0].0) {
//                     if !road.1.0.1.is_empty() {
//                         let mut destination_position = Vec3::ZERO;
//                         match logistic_unit.1.destination[0].1.0 {
//                             UnitTypes::Regular => {
//                                 if let Some(platoon) = army.regular_platoons.get(&logistic_unit.1.destination[0].1.1) {
//                                     if let Ok (consumer) = supply_consumers.get(platoon.2) {
//                                         destination_position = consumer.1.translation;
//                                     }
//                                 }
//                             },
//                             UnitTypes::Shock => {
    
//                             },
//                             UnitTypes::Armored => {
    
//                             },
//                             UnitTypes::None => {},
//                         }

//                         let mut nearest_road = (f32::INFINITY, Entity::PLACEHOLDER, Vec::<Entity>::new() );
//                         for road_entity in road.1.0.1.iter() {
//                             if let Ok(another_road) = roads.get(*road_entity) {
//                                 let distance_to_current_road = destination_position.distance(another_road.1.0.0.road_center);
//                                 if distance_to_current_road < nearest_road.0 {
//                                     nearest_road = (distance_to_current_road, another_road.0, another_road.1.0.1.clone());
//                                 }
//                             }
//                         }
    
//                         logistic_unit.1.destination[0].0 = nearest_road.1;

//                         let mut second_nearest_road = f32::INFINITY;
//                         for road_entity in nearest_road.2.iter() {
//                             if let Ok(another_road) = roads.get(*road_entity) {
//                                 let distance_to_second_road = destination_position.distance(another_road.1.0.0.road_center);
//                                 if distance_to_second_road < second_nearest_road {
//                                     second_nearest_road = distance_to_second_road;
//                                 }
//                             }
//                         }

//                         if nearest_road.0 <= second_nearest_road {
//                             logistic_unit.1.is_on_final_road = true;
//                         }
//                     }
//                 }
//             }

//             match logistic_unit.1.destination[0].1.0 {
//                 UnitTypes::Regular => {
//                     if let Some(platoon) = army.regular_platoons.get(&logistic_unit.1.destination[0].1.1) {
//                         if let Ok (consumer) = supply_consumers.get(platoon.2) {
//                             let current_distance_to_destination = logistic_unit.2.translation.distance(consumer.1.translation);
//                             if logistic_unit.1.last_distance_to_destination >= current_distance_to_destination {
//                                 logistic_unit.1.last_distance_to_destination = current_distance_to_destination;
//                             } else if logistic_unit.1.is_on_final_road && logistic_unit.0.path.is_empty() {
//                                 logistic_unit.1.destination[0].2 = false;
//                                 logistic_unit.1.destination[0].0 = Entity::PLACEHOLDER;

//                                 let async_task_pools.logistic_pathfinding_pool = AsyncComputeTaskPool::get();
//                                 let nav_mesh_lock = nav_mesh.get();
                
//                                 let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
//                                     nav_mesh_lock.clone(),
//                                     nav_mesh_settings.clone(),
//                                     logistic_unit.2.translation,
//                                     consumer.1.translation,
//                                     Some(100.),
//                                     Some(&[1.0, 0.5]),
//                                     logistic_unit.3,
//                                 ));
                
//                                 pathfinding_task.tasks.push(task);
//                             }
//                         }
//                     }
//                 },
//                 UnitTypes::Shock => {

//                 },
//                 UnitTypes::Armored => {

//                 },
//                 UnitTypes::None => {},
//             }
//         }
//         else if logistic_unit.0.path != vec![] && !logistic_unit.1.destination[0].2 && logistic_unit.1.destination[0].0 == Entity::PLACEHOLDER {
//             match logistic_unit.1.destination[0].1.0 {
//                 UnitTypes::Regular => {
//                     if let Some(platoon) = army.regular_platoons.get(&logistic_unit.1.destination[0].1.1) {
//                         if let Ok (consumer) = supply_consumers.get(platoon.2) {
//                             if logistic_unit.0.path[logistic_unit.0.path.len() - 1].distance(consumer.1.translation) >= 10. {
//                                 let async_task_pools.logistic_pathfinding_pool = AsyncComputeTaskPool::get();
//                                 let nav_mesh_lock = nav_mesh.get();
                
//                                 let task = async_task_pools.logistic_pathfinding_pool.spawn(async_path_find(
//                                     nav_mesh_lock.clone(),
//                                     nav_mesh_settings.clone(),
//                                     logistic_unit.2.translation,
//                                     consumer.1.translation,
//                                     Some(100.),
//                                     Some(&[1.0, 0.5]),
//                                     logistic_unit.3,
//                                 ));
                
//                                 pathfinding_task.tasks.push(task);
//                             }

//                             if logistic_unit.2.translation.distance(consumer.1.translation) < 10. {
//                                 commands.entity(logistic_unit.3).despawn();//Цель достигнута типа
//                             }
//                         }
//                     }
//                 },
//                 UnitTypes::Shock => {

//                 },
//                 UnitTypes::Armored => {

//                 },
//                 UnitTypes::None => {},
//             }
//         }

//         if !logistic_unit.0.path.is_empty() && !is_moved {
//             let next_point = logistic_unit.0.path[0];
//             let mut direction = (next_point - logistic_unit.2.translation).normalize();
//             direction.y = 0.0;
//             logistic_unit.2.translation += direction * speed * time.delta_seconds();

//             if logistic_unit.2.translation.xz().distance(next_point.xz()) < 0.1 {

//                 logistic_unit.0.path.remove(0);
//             }
//         }
//     }
// }