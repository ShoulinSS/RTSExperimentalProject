use core::alloc;
use std::{fs::read_link, hash::Hasher, string, sync::{mpsc::channel, Arc, RwLock}};
use bevy::{audio::{PlaybackMode, Volume}, ecs::event, input::mouse, log::tracing_subscriber::fmt::format::Compact, math::{self, NormedVectorSpace, VectorSpace}, pbr::{ExtendedMaterial, NotShadowCaster}, prelude::*, reflect::{array_hash, Map}, tasks::{available_parallelism, futures_lite::future, AsyncComputeTaskPool, Task}, transform::commands, utils::{hashbrown::{hash_map::{Iter, IterMut}, HashMap}, HashSet}};
use bevy_egui::egui::load::TextureLoadResult;
use bevy_mod_raycast::{cursor::CursorRay, prelude::Raycast};
use bevy_quinnet::{client::{client_connecting, QuinnetClient}, server::{self, QuinnetServer}};
use bevy_rapier3d::{na::{ComplexField, Quaternion, distance}, parry::query::point, prelude::{Collider, CollisionGroups, Group, KinematicCharacterController, LockedAxes}, rapier::crossbeam::channel};
use oxidized_navigation_serializable::{query::find_path, tiles::NavMeshTiles, NavMesh, NavMeshAffector, NavMeshAreaType, NavMeshSettings};
use rand::Rng;
use std::hash::Hash;
use bevy_tasks::TaskPool;
use serde::{Deserialize, Serialize};

use crate::{FOG_TEXTURE_SIZE, GameStage, GameStages, HUMAN_RESOURCE_COLOR, MATERIALS_COLOR, PlayerData, WORLD_SIZE, components::{asset_manager::{AttackVisualisationAssets, ChangeMaterial, ExplosionComponent, InstancedMaterials, LOD, OtherAssets, TeamMaterialExtension, TrailComponent, TrailEmmiterComponent}, building::{CONSTRUCTION_PROGRESS_COLOR, ConstructionProgressBar, DeconstructableBuilding, DontTouch, HumanResourcesDisplay, MaterialsDisplay, MaterialsProductionComponent, SettlementComponent, SwitchableBuilding, ToDeconstruct}, camera::{self, CameraComponent}, logistics::LogisticUnitComponent, ui_manager::{ArtilleryUnitSelectedEvent, BattalionSelectionEvent, BrigadeSelectionEvent, CompanySelectionEvent, PlatoonSelectionEvent, RegimentSelectionEvent, TransportDisembarkEvent, UiBlocker, UiButtonNodes}}};

use super::{building::{ArtilleryBundle, BuildingBlueprint, BuildingConstructionSite, BuildingsBundles, CoverComponent, EngineerBundle, IFVBundle, InfantryBarracksBundle, InfantryProducer, LogisticHubBundle, ProductionQueue, ProductionState, ResourceMinerBundle, SoldierBundle, SuppliesProductionComponent, TankBundle, UnactivatedBlueprints, UnitBundles, UnitProductionBuildingComponent, VehicleFactoryBundle, VehiclesProducer}, camera::MoveOrderEvent, logistics::ResourceZone, network::{self, ClientList, ClientMessage, EntityMaps, NetworkStatus, NetworkStatuses, ServerMessage}, ui_manager::{CancelArtilleryTargets, GameStartedEvent, SquadSelectionEvent, ProductionStateChanged, ToggleArtilleryDesignation}};

#[derive(Default, Resource)]
pub struct AsyncPathfindingTasks {
    pub tasks: Vec<Task<Option<(Vec<Vec3>, Entity)>>>,
}

#[derive(Debug)]
pub struct LimitedHashMap<K, V, const N: usize> {
    map: HashMap<K, V>,
}

impl<K, V, const N: usize> LimitedHashMap<K, V, N>
where
    K: Hash + Eq,
{
    pub fn new() -> Self {
        LimitedHashMap {
            map: HashMap::default(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, &'static str> {
        if self.is_full() {
            return Err("Exceeded maximum size of LimitedHashMap");
        }
        Ok(self.map.insert(key, value))
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.map.remove(key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_full(&self) -> bool {
        self.map.len() >= N
    }

    pub fn max_len(&self) -> usize {
        N
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.map.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.map.iter_mut()
    }
}

pub struct LimitedHashSet<T, const N: usize> {
    pub set: HashSet<T>,
}

impl<T: std::hash::Hash + Eq, const N: usize> LimitedHashSet<T, N> {
    pub fn new() -> Self {
        LimitedHashSet {
            set: HashSet::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> Result<(), &str> {
        if self.set.len() >= N {
            Err("Exceeded maximum size of LimitedHashSet")
        } else {
            self.set.insert(value);
            Ok(())
        }
    }

    pub fn remove(&mut self, value: &T) -> bool {
        self.set.remove(value)
    }

    pub fn contains(&self, value: &T) -> bool {
        self.set.contains(value)
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_full(&self) -> bool {
        self.set.len() >= N
    }

    pub fn capacity(&self) -> usize {
        N
    }
}

#[derive(Eq, Hash, PartialEq, Clone, Default)]
pub struct LimitedNumber<const S: i32, const L: usize> {
    value: i32,
}

impl<const S: i32, const L: usize> LimitedNumber<S, L> {
    pub fn new() -> Self {
        LimitedNumber { value: S }
    }

    pub fn from_value(start_value: i32) -> Self {
        LimitedNumber { value: start_value }
    }

    pub fn next(&mut self) -> bool {
        self.value += 1;
        if self.value >= S + L as i32 {
            self.value = S;
            return true;
        }
        else{
            return false;
        }
    }

    pub fn previous(&mut self) -> bool {
        self.value -= 1;
        if self.value <= -1 {
            self.value = S + L as i32 - 1;
            return true;
        }
        else{
            return false;
        }
    }

    pub fn get_value(&self) -> i32 {
        self.value
    }

    pub fn set_value(&mut self, value: i32) {
        self.value = value;
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum CompanyTypes {
    Regular,
    Shock,
    Armored,
    Artillery,
    Engineer,
    None,
}

#[derive(Clone, Copy)]
pub enum UnitTypes {
    Infantry,
    LightVehicle,
    HeavyVehicle,
    Building,
    None,
}

pub const REGULAR_SQUAD_SIZE: usize = 8;
pub const SPECIALISTS_PER_REGULAR_SQUAD: usize = 1;
pub struct RegularSquad(pub (LimitedHashSet<Entity, REGULAR_SQUAD_SIZE>, LimitedHashSet<Entity, SPECIALISTS_PER_REGULAR_SQUAD>));

#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableRegularSquad(pub (Vec<Entity>, Vec<Entity>));

pub const SHOCK_SQUAD_SIZE: usize = 8;
pub const SPECIALISTS_PER_SHOCK_SQUAD: usize = 1;
pub struct ShockSquad(pub (LimitedHashSet<Entity, SHOCK_SQUAD_SIZE>, LimitedHashSet<Entity, SPECIALISTS_PER_SHOCK_SQUAD>));

#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableShockSquad(pub (Vec<Entity>, Vec<Entity>));

pub const ARMORED_SQUAD_SIZE: usize = 2;
pub struct ArmoredSquad(pub LimitedHashSet<Entity, ARMORED_SQUAD_SIZE>);

#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableArmoredSquad(pub Vec<Entity>);

pub const PLATOON_SIZE: usize = 3;//27 units

pub const COMPANY_SIZE: usize = 3;//81 units

pub const BATTALION_SIZE: usize = 3;//243 units

pub const REGIMENT_SIZE: usize = 3;//729 units

pub const ARMY_SIZE: usize = 3;//2187 units

pub const MAX_SQUAD_COUNT: i32 = 243;//2187 units

pub const START_REGULAR_SQUADS_AMOUNT: i32 = 81;  // \
pub const START_SHOCK_SQUADS_AMOUNT: i32 = 81;    //  } must be 243
pub const START_ARMORED_SQUADS_AMOUNT: i32 = 81;  // /

pub const START_ARTILLERY_UNITS_COUNT: i32 = 6;
pub const START_ENGINEERS_COUNT: i32 = 10;

#[derive(Resource)]
pub struct Armies(pub HashMap<i32, ArmyObject>);

pub struct ArmyObject{
    pub regular_squads: HashMap<(i32, i32, i32, i32, i32), (RegularSquad, String, Entity)>,
    pub shock_squads: HashMap<(i32, i32, i32, i32, i32), (ShockSquad, String, Entity)>,
    pub armored_squads: HashMap<(i32, i32, i32, i32, i32), (ArmoredSquad, String, Entity)>,
    pub artillery_units: (HashMap<i32, ((Option<Entity>, String), Entity)>, Entity),
    pub engineers: HashMap<i32, ((Option<Entity>, String), Entity)>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SerializableArmyObject{
    pub regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))>,
    pub shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))>,
    pub armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))>,
    pub artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity),
    pub engineers: Vec<(i32, ((Option<Entity>, String), Entity))>,
}

#[derive(Event)]
pub struct UnitDeathEvent {
    pub dead_unit_data: (
        i32,                                                                            //team
        ((i32, i32), (CompanyTypes, (i32, i32, i32, i32, i32, i32, i32), String)),      //unit data
        Option<Entity>,                                                                 //cover entity
        Entity,                                                                         //unit entity
        Transform,                                                                      //unit transform
        bool,                                                                           //should leave a corpse
    ),
}

#[derive(Event)]
pub struct UnitNeedsToBeUncovered {
    pub cover_entity: Entity,
    pub unit_entity: Entity,
}

#[derive(Resource)]
pub struct TargetPosition {
    pub position: Vec3
}

#[derive(Resource)]
pub struct UnitsTileMap {
    pub tiles: HashMap<i32, HashMap<(i32, i32), HashMap<Entity, (Vec3, UnitTypes)>>>,
}

pub const TILE_SIZE: f32 = 50.;

#[derive(Resource)]
pub struct SelectedUnits {
    pub platoons: HashMap<(CompanyTypes, (i32, i32, i32, i32, i32)), Vec<Entity>>,
}

#[derive(Resource)]
pub struct AsyncTaskPools {
    pub manual_pathfinding_pool: TaskPool,
    pub logistic_pathfinding_pool: TaskPool,
    pub extra_pathfinding_pool: TaskPool,
}

pub fn add_selected_units(
    units: Vec<Entity>,
    selected_units: &mut ResMut<SelectedUnits>,
    commands: &mut Commands,
    units_q: &Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
){
    for unit_entity in units.iter() {
        if !selected_units.platoons.values().any(|units| units.contains(&unit_entity)) {
            if let Ok(unit) = units_q.get(*unit_entity){

                selected_units.platoons.entry(
                    (
                        unit.2.unit_data.1.0,
                        (
                            unit.2.unit_data.1.1.0,
                            unit.2.unit_data.1.1.1,
                            unit.2.unit_data.1.1.2,
                            unit.2.unit_data.1.1.3,
                            unit.2.unit_data.1.1.4,
                        )
                    )
                ).or_insert_with(Vec::new).push(unit.1);

                commands.entity(unit.1).try_insert(SelectedUnit);
            }
        }
    }
}

pub fn clear_selected_units(
    selected_units: &mut ResMut<SelectedUnits>,
    commands: &mut Commands,
    units_q: &Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
){
    for platoon in selected_units.platoons.clone(){
        for unit_entity in platoon.1.iter() {
            if units_q.get(*unit_entity).is_ok() {
                commands.entity(*unit_entity).remove::<SelectedUnit>();
            }
        }
    }
    selected_units.platoons.clear();
}

#[derive(Component, Clone)]
pub struct UnitComponent{
    pub path: Vec<Vec3>,
    pub start_position: Vec3,
    pub quantized_destination: Option<Vec3>,
    pub speed: f32,
    pub waypoint_radius: f32,
    pub elapsed: f32,
    pub inv_duration: f32,
    pub last_position: Vec3,
    pub stuck_count: i32,
}

#[derive(Component, Clone)]
pub struct SelectableUnit;

#[derive(Clone)]
pub enum AttackTypes{
    Direct(
        i32,    //damage
        f32,    //accuracy
        DamageTypes,
    ),
    BallisticProjectile(
        f32,    //max height
        usize,  //ballistic points num
        f32,    //speed
        f32,    //points check factor
        f32,    //accuracy
        (
            i32,//direct damage
            DamageTypes,
        ),
        (
            f32,    //AOE
            i32,    //splash damage
            DamageTypes,
        ),
        Vec3,   //projectile spawn position
    ),
    HomingProjectile(
        f32,    //speed
        f32,    //waypoints check factor
        usize,  //prediction iterations
        f32,    //prediction tolerance
        (
            i32,//direct damage
            DamageTypes,
        ),
        (
            f32,    //AOE
            i32,    //splash damage
            DamageTypes,
        ),
        Vec3,   //projectile spawn position
    ),
    None,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum DamageTypes{
    AntiInfantry,
    AntiTank,
    AntiBuilding,
    Universal,
}

#[derive(Clone)]
pub enum AttackAnimationTypes{
    LowCaliber(Vec3),
    HighCaliber(Vec3),
    MissileLaunch(Vec3),
    TankCannon(Vec3),
    None(Vec3),
}

impl PartialEq for AttackAnimationTypes {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for AttackAnimationTypes {}

impl Hash for AttackAnimationTypes {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}

#[derive(Component, Clone)]
pub struct CombatComponent{
    pub team: i32,
    pub current_health: i32,
    pub max_health: i32,
    pub unit_type: UnitTypes,
    pub attack_type: AttackTypes,
    pub attack_animation_type: AttackAnimationTypes,
    pub attack_frequency: u128,
    pub attack_elapsed_time: u128,
    pub detection_range: f32,
    pub attack_range: f32,
    pub enemies: Vec<(Entity, f32)>,
    pub is_static: bool,
    pub unit_data: (
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
}

#[derive(Component)]
pub struct MovingToCover{
    pub cover_entity: Entity,
    pub cover_position: Vec3,
}

#[derive(Component)]
pub struct Covered{
    pub cover_efficiency: f32,
    pub cover_entity: Entity,
    pub original_y: f32,
}

#[derive(Component, Clone)]
pub struct SuppliesConsumerComponent {
    pub supplies_capacity: i32,
    pub supplies: i32,
    pub consume_rate: i32,
    pub supply_range: f32,
    pub supply_frequency: u128,
    pub elapsed_time: u128,
}

#[derive(Component, Clone)]
pub struct EngineerComponent {
    pub build_power: i32,
}

#[derive(Component)]
pub struct BusyEngineer(pub EngineerActions);

pub enum EngineerActions{
    ActivateBlueprint((Vec3, Entity, f32)),
    Construction((Vec3, Entity, f32)),
    Deconstruction((Vec3, Entity, f32)),
}

#[derive(Component)]
pub struct SquadLeader(pub (CompanyTypes, (i32, i32, i32, i32, i32)));

#[derive(Component)]
pub struct SelectedUnit;

#[derive(Component)]
pub struct NeedToMove;

#[derive(Component)]
pub struct StoppedMoving;

#[derive(Component, Clone)]
pub struct ArtilleryUnit {
    pub peak_trajectory_height: f32,
    pub trajectory_points: usize,
    pub projectile_waypoints_check_factor: f32,
    pub shell_speed: f32,
    pub max_range: f32,
    pub accuracy: f32,
    pub reload_time: u128,
    pub elapsed_reload_time: u128,
    pub direct_damage: (i32, DamageTypes),
    pub splash_damage: (f32, i32, DamageTypes),
}

#[derive(Component)]
pub struct BallisticProjectile {
    pub path: Vec<Vec3>,
    pub start_position: Vec3,
    pub target_position: Vec3,
    pub speed: f32,
    pub waypoint_radius: f32,
    pub elapsed: f32,
    pub inv_duration: f32,
    pub direct_damage: (i32, DamageTypes),
    pub splash_damage: (f32, i32, DamageTypes),
}

#[derive(Resource)]
pub struct IsArtilleryDesignationActive(pub bool);

#[derive(Component)]
pub struct ArtilleryNeedsToFire(pub Vec3);

#[derive(Resource)]
pub struct IsUnitDeselectionAllowed(pub bool);

#[derive(Resource)]
pub struct IsUnitSelectionAllowed(pub bool);

#[derive(Event)]
pub struct ArtilleryOrderGiven;

#[derive(Event)]
pub struct ExplosionEvent(pub (Vec3, (i32, DamageTypes), (f32, i32, DamageTypes)));

#[derive(Component)]
pub struct HomingProjectile {
    pub speed: f32,
    pub hit_check_factor: f32,
    pub target_entity: Entity,
    pub targets_last_position: Vec3,
    pub max_prediction_caluclation_iterations: usize,
    pub prediction_tolerance: f32,
    pub direct_damage: (i32, DamageTypes),
    pub splash_damage: (f32, i32, DamageTypes),
}

#[derive(Component)]
pub struct DeleteAfterStart;

pub const QUANTIZATION_THRESHOLD: f32 = 300.;//needs to be fixed

pub fn process_manual_pathfinding(
    mut units_q: Query<(&Transform, Entity, &mut UnitComponent), With<SelectedUnit>>,
    target: Res<TargetPosition>,
    selected_units: Res<SelectedUnits>,
    formation: ResMut<camera::Formation>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut event_reader: EventReader<camera::MoveOrderEvent>,
){
    for _event in event_reader.read(){
        if formation.is_formation_active {
            let mut desired_distance_between_points = 5.;
            let mut formation_length =0.;

            let mut counter = 0;
            for point in formation.points.iter().skip(1) {
                formation_length += point.distance(formation.points[counter]);

                counter += 1;
            }

            let selected_units_count: usize = selected_units.platoons.values().map(|platoon| platoon.len()).sum();
            if formation_length / selected_units_count as f32 > desired_distance_between_points {
                desired_distance_between_points = formation_length / selected_units_count as f32 * 0.99;
            }

            let mut formation_positions: Vec<Vec3> = Vec::new();
            let mut last_point = formation.points[0];
            formation_positions.push(last_point);
            let mut distance_accumulator = 0.;

            for point in formation.points.iter() {
                distance_accumulator += point.distance(last_point);

                if distance_accumulator >= desired_distance_between_points {
                    for _i in 0..(distance_accumulator / desired_distance_between_points) as i32 {
                        distance_accumulator -= desired_distance_between_points;

                        let direction = (point.clone() - last_point).normalize();

                        let new_position = formation_positions[formation_positions.len() - 1] + direction * desired_distance_between_points;

                        formation_positions.push(new_position);
                    }
                }

                last_point = *point;
            }

            last_point = formation_positions[0];
            let platoons_spacing = 10.;
            distance_accumulator = 0.;
            let mut sorted_platoons: Vec<Vec<Entity>> = Vec::new();
            let mut formation_segments: Vec<Vec<Vec3>> = Vec::new();
            let mut formation_positions_clone = formation_positions.clone();
            formation_positions_clone.remove(0);
            let desired_allocated_length = 26. + platoons_spacing;
            let calculated_allocated_length = (formation_length + platoons_spacing) / selected_units.platoons.len() as f32;
            let actual_allocated_length;

            if calculated_allocated_length < desired_allocated_length {
                let platoons_in_row = (formation_length / desired_allocated_length) as i32;
                actual_allocated_length = (formation_length + platoons_spacing) / platoons_in_row as f32;
            }
            else {
                actual_allocated_length = calculated_allocated_length;
            }

            for (index, _platoon) in selected_units.platoons.iter().enumerate(){
                let mut positions_to_delete = 0;

                formation_segments.push(Vec::new());
                formation_segments[index].push(last_point);

                for position in formation_positions_clone.iter() {
                    distance_accumulator += position.distance(last_point);
                    
                    if distance_accumulator <= actual_allocated_length{
                        positions_to_delete += 1;

                        formation_segments[index].push(*position);
                    }
                    else{
                        positions_to_delete += 1;
                        last_point = *position;
                        distance_accumulator -= actual_allocated_length;
                        break;
                    }

                    last_point = *position;
                }

                for _i in 0..positions_to_delete{
                    formation_positions_clone.remove(0);
                }

                if formation_positions_clone.is_empty() {
                    formation_positions_clone = formation_positions.clone();

                    let mut largest_platoon = 0;
                    for platoon in selected_units.platoons.iter(){
                        if platoon.1.len() > largest_platoon {
                            largest_platoon = platoon.1.len();
                        }
                    }

                    let largest_platoon_rows = (largest_platoon + formation_segments[0].len() - 1) / formation_segments[0].len();
                    let offset = largest_platoon_rows as f32 * desired_distance_between_points + platoons_spacing;

                    let direction = (formation_positions[formation_positions.len() - 1] - formation_positions[0]).normalize();
                    let cross = Vec3::new(direction.z * -1., direction.y, direction.x);

                    for position in formation_positions_clone.iter_mut(){
                        position.x += offset * cross.x;
                        position.z += offset * cross.z;
                    }

                    formation_positions = formation_positions_clone.clone();

                    last_point = formation_positions_clone[0];
                    formation_positions_clone.remove(0);
                    distance_accumulator = 0.;
                }
            }

            let mut used_platoons: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

            for segment in formation_segments.iter(){
                let mut nearest_platoon = ((CompanyTypes::None, (0, 0, 0, 0, 0)), f32::INFINITY);
                for platoon in selected_units.platoons.iter(){
                    if !used_platoons.contains(platoon.0) {
                        if let Ok(unit) = units_q.get(platoon.1[0]) {
                            let current_distance = segment[0].distance(unit.0.translation);
                            if current_distance < nearest_platoon.1 {
                                nearest_platoon = (*platoon.0, current_distance);
                            }
                        }
                    }
                }
                
                used_platoons.push(nearest_platoon.0);

                if let Some(platoon) = selected_units.platoons.get(&nearest_platoon.0){
                    sorted_platoons.push(platoon.clone());
                }
            }

            distance_accumulator = 0.;

            for (index, platoon) in sorted_platoons.iter().enumerate(){
                let mut allowed_positions: Vec<Vec3> = Vec::new();
                let mut last_position = formation_segments[index][0];
                let mut point_skip_factor = 0;

                if platoon.len() < formation_segments[index].len() {
                    point_skip_factor = formation_segments[index].len() / platoon.len();
                }

                let mut counter = point_skip_factor;
                for position in formation_segments[index].iter(){
                    counter += 1;
                    distance_accumulator += position.distance(last_position);

                    if distance_accumulator <= actual_allocated_length - platoons_spacing{
                        if point_skip_factor == 0{
                            allowed_positions.push(*position);
                        } else if counter >= point_skip_factor {
                            counter = 0;

                            allowed_positions.push(*position);
                        }

                        last_position = *position;
                    }
                    else{
                        break;
                    }
                }

                distance_accumulator = 0.;

                counter = 0;
                if allowed_positions.len() > 0 {
                    for unit_entity in platoon.iter(){
                        if let Ok(mut unit) = units_q.get_mut(*unit_entity) {
                            unit.2.path = Vec::new();

                            let mut destination = allowed_positions[counter];

                            // if unit.0.translation.distance(allowed_positions[counter]) > QUANTIZATION_THRESHOLD {
                            //     destination = unit.0.translation + (allowed_positions[counter] - unit.0.translation).normalize() * QUANTIZATION_THRESHOLD;
                            //     unit.2.quantized_destination = Some(allowed_positions[counter]);
                            // } else {
                            //     unit.2.quantized_destination = None;
                            // }

                            unstarted_tasks.0.push((
                                TaskPoolTypes::Manual,
                                (
                                    unit.0.translation,
                                    destination,
                                    Some(100.),
                                    unit.1,
                                ),
                            ));
    
                            counter +=1;
    
                            if counter >= allowed_positions.len() {
                                counter = 0;
    
                                let platoon_formation_direction = (allowed_positions[allowed_positions.len() - 1] - allowed_positions[0]).normalize();
                                let cross= Vec3::new(platoon_formation_direction.z * -1., platoon_formation_direction.y, platoon_formation_direction.x);
                                for position in allowed_positions.iter_mut() {
                                    position.x += desired_distance_between_points * cross.x;
                                    position.z += desired_distance_between_points * cross.z;
                                }
                            }
                        }
                    }
                }
            }
        }
        else{
            let mut counter = 0;
            let mut operation_counter = 0;
            let mut operation_number = 1;   //\/
            let mut z_minus = 2;            //1
            let mut x_minus = 2;            //2
            let mut z_plus = 3;             //3
            let mut x_plus = 3;             //4
            let mut largest_platoon = 0;

            for platoon in selected_units.platoons.iter(){
                if platoon.1.len() > largest_platoon {
                    largest_platoon = platoon.1.len();
                }
            }
            let mut offset = (largest_platoon as f32).sqrt().floor() * 5. + 10.;

            let mut origin_position = target.position;
            let mut platoons_positions: Vec<Vec3> = Vec::new();

            for _platoon in selected_units.platoons.iter(){
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

                platoons_positions.push(origin_position);

                counter += 1;
            }

            let mut selected_platoons_iter = selected_units.platoons.iter();

            for position in platoons_positions.iter(){
                origin_position = *position;

                counter = 0;
                operation_counter = 0;
                operation_number = 1;   //\/
                z_minus = 2;            //1
                x_minus = 2;            //2
                z_plus = 3;             //3
                x_plus = 3;             //4
                offset = 5.;

                if let Some(platoon) = selected_platoons_iter.next(){
                    for unit_entity in platoon.1.iter(){
                        if let Ok(mut unit) = units_q.get_mut(*unit_entity){
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

                            unit.2.path = Vec::new();

                            let mut destination = origin_position;

                            // if unit.0.translation.distance(origin_position) > QUANTIZATION_THRESHOLD {
                            //     destination = unit.0.translation + (origin_position - unit.0.translation).normalize() * QUANTIZATION_THRESHOLD;
                            //     unit.2.quantized_destination = Some(origin_position);
                            // } else {
                            //     unit.2.quantized_destination = None;
                            // }

                            unstarted_tasks.0.push((
                                TaskPoolTypes::Manual,
                                (
                                    unit.0.translation,
                                    destination,
                                    Some(100.),
                                    unit.1,
                                ),
                            ));
            
                            counter += 1;
                        }
                    }
                }
            }
        }
    }
}

pub enum TaskPoolTypes {
    Manual,
    Logistic,
    Extra,
}

#[derive(Resource)]
pub struct UnstartedPathfindingTasksPool(
    pub Vec<(
        TaskPoolTypes,
        (
            Vec3,
            Vec3,
            Option<f32>,
            Entity,
        ),
    )>
);

pub fn pathfinding_tasks_starter(
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    nav_mesh: Res<NavMesh>,
    nav_mesh_settings: Res<NavMeshSettings>,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    async_task_pools: Res<AsyncTaskPools>,
){
    if !unstarted_tasks.0.is_empty() {
        let nav_mesh_lock = nav_mesh.get();

        for unstarted_task in unstarted_tasks.0.iter() {
            let task;

            match unstarted_task.0 {
                TaskPoolTypes::Manual => {
                    task = async_task_pools.manual_pathfinding_pool.spawn(async_path_find(
                        nav_mesh_lock.clone(),
                        nav_mesh_settings.clone(),
                        unstarted_task.1.0,
                        unstarted_task.1.1,
                        unstarted_task.1.2,
                        Some(&[1.0, 1.5]),
                        unstarted_task.1.3,
                    ));
                },
                TaskPoolTypes::Logistic => {
                    task = async_task_pools.manual_pathfinding_pool.spawn(async_path_find(
                        nav_mesh_lock.clone(),
                        nav_mesh_settings.clone(),
                        unstarted_task.1.0,
                        unstarted_task.1.1,
                        unstarted_task.1.2,
                        Some(&[1.0, 0.1]),
                        unstarted_task.1.3,
                    ));
                },
                TaskPoolTypes::Extra => {
                    task = async_task_pools.manual_pathfinding_pool.spawn(async_path_find(
                        nav_mesh_lock.clone(),
                        nav_mesh_settings.clone(),
                        unstarted_task.1.0,
                        unstarted_task.1.1,
                        unstarted_task.1.2,
                        Some(&[1.0, 1.5]),
                        unstarted_task.1.3,
                    ));
                },
            }

            pathfinding_task.tasks.push(task);
        }

        unstarted_tasks.0.clear();
    }
}

pub fn poll_pathfinding_tasks_system(
    mut commands: Commands,
    mut pathfinding_task: ResMut<AsyncPathfindingTasks>,
    mut units_q: Query<(Entity, &mut UnitComponent)>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    // mut event_writer: (
    //     EventWriter<UnsentClientMessage>,
    // ),
){
    if pathfinding_task.tasks.len() > 0 {
        pathfinding_task.tasks.retain_mut(|task| {
            if task.is_finished() {
                if let Some((path, entity)) = future::block_on(future::poll_once(task)).unwrap_or(None) {
                    if let Ok((unit_entity, mut unit_component)) = units_q.get_mut(entity){
                        match network_status.0 {
                            NetworkStatuses::SinglePlayer => {
                                unit_component.path = path;
                                unit_component.elapsed = 0.;
                                commands.entity(unit_entity).try_insert(NeedToMove);
                            },
                            NetworkStatuses::Host => {
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitPathInserted {
                                        server_entity: unit_entity,
                                        path: path.clone(),
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }

                                unit_component.path = path;
                                commands.entity(unit_entity).try_insert(NeedToMove);
                            },
                            NetworkStatuses::Client => {
                                if let Some(server_entity) = entity_maps.client_to_server.get(&unit_entity) {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::UnitPathInsertRequest {
                                            entity: *server_entity,
                                            path: path.clone(),
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                        }
                    }
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });
    }
}

pub async fn async_path_find(
    nav_mesh_lock: Arc<RwLock<NavMeshTiles>>,
    nav_mesh_settings: NavMeshSettings,
    start_pos: Vec3,
    end_pos: Vec3,
    position_search_radius: Option<f32>,
    area_cost_multiplier: Option<&[f32]>,
    entity: Entity,
) -> Option<(Vec<Vec3>, Entity)> {
    if end_pos.x < WORLD_SIZE / 2. && end_pos.z < WORLD_SIZE / 2. {
        let Ok(nav_mesh) = nav_mesh_lock.read() else {
            return None;
        };
    
        match find_path(
            &nav_mesh,
            &nav_mesh_settings,
            start_pos,
            end_pos,
            position_search_radius,
            area_cost_multiplier,
        ) {
            Ok(path) => {
                return Some((path, entity));
            }
            Err(error) => error!("Pathfinding error (╯°□°）╯︵ ┻━┻: {:?}", error),
        }
    }

    return Some((vec![], Entity::PLACEHOLDER));
}

pub fn process_agents_movement(
    mut units_q: Query<(&mut UnitComponent, &mut Transform, Entity, Option<&mut KinematicCharacterController>, Option<&CombatComponent>, Option<&LogisticUnitComponent>),
    (With<NeedToMove>, Without<Covered>)>,
    mut event_writer: EventWriter<UnitDeathEvent>,
    mut commands: Commands,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
){
    for(mut unit_component,mut unit_transform, unit_entity, controller_option, combat_component_option, logistic_unit_option) in units_q.iter_mut(){
        if !unit_component.path.is_empty(){
            let unit_position = unit_transform.translation;
            let target_position = unit_component.path[0];

            if unit_component.elapsed == 0. {
                unit_transform.look_at(
                    Vec3::new(
                        target_position.x,
                        unit_position.y,
                        target_position.z,
                    ),
                    Vec3::Y,
                );
                
                unit_component.start_position = unit_position;
                let distance = unit_component.start_position.xz().distance(target_position.xz());

                if distance <= unit_component.waypoint_radius {
                    unit_component.path.remove(0);
                } else {
                    let duration = distance / unit_component.speed;
                    unit_component.inv_duration = 1. / duration;

                    unit_component.elapsed += time.delta_seconds();
                    let t = unit_component.elapsed * unit_component.inv_duration;

                    let new_pos = if t >= 1. {
                        target_position
                    } else {
                        unit_component.start_position.lerp(target_position, t)
                    };

                    let mut delta = new_pos - unit_position;
                    delta.y = 0.;

                    if let Some(mut controller) = controller_option {
                        controller.translation = Some(delta);
                    } else {
                        unit_transform.translation = new_pos;
                    }
                }
            } else {
                if unit_position.xz().distance(target_position.xz()) <= unit_component.waypoint_radius {
                    unit_component.path.remove(0);
                    unit_component.elapsed = 0.;
                } else {
                    unit_component.elapsed += time.delta_seconds();
                    let t = unit_component.elapsed * unit_component.inv_duration;

                    let new_pos = if t >= 1.0 {
                        target_position
                    } else {
                        unit_component.start_position.lerp(target_position, t)
                    };

                    let mut delta = new_pos - unit_position;
                    delta.y = 0.;

                    if let Some(mut controller) = controller_option {
                        controller.translation = Some(delta);
                    } else {
                        unit_transform.translation = new_pos;
                    }
                }
            }

            if unit_component.path.is_empty() {
                if let Some(destination) = unit_component.quantized_destination {
                    let area_cost_multiplier: Option<&[f32]>;
                    if let Some(_) = logistic_unit_option {
                        area_cost_multiplier = Some(&[10.0, 0.1]);
                    } else {
                        area_cost_multiplier = Some(&[1.0, 1.5]);
                    }

                    let mut new_destination = destination;

                    if unit_transform.translation.distance(destination) > QUANTIZATION_THRESHOLD {
                        new_destination = unit_transform.translation + (destination - unit_transform.translation).normalize() * QUANTIZATION_THRESHOLD;
                    } else {
                        unit_component.quantized_destination = None;
                    }

                    unstarted_tasks.0.push((
                        TaskPoolTypes::Manual,
                        (
                            unit_transform.translation,
                            new_destination,
                            Some(100.),
                            unit_entity,
                        ),
                    ));
                }
                
                commands.entity(unit_entity).remove::<NeedToMove>();
                commands.entity(unit_entity).try_insert(StoppedMoving);

                unit_component.elapsed = 0.;

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                            server_entity: unit_entity,
                            new_position: unit_transform.translation,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            if unit_component.last_position == unit_transform.translation {
                unit_component.stuck_count += 1;

                if unit_component.stuck_count >= 10 {
                    if let Some(combat_component) = combat_component_option {
                        event_writer.send(UnitDeathEvent{
                            dead_unit_data: (
                                combat_component.team,
                                combat_component.unit_data.clone(),
                                None,
                                unit_entity,
                                *unit_transform,
                                false,
                            ),
                        });
                    } else {
                        event_writer.send(UnitDeathEvent{
                            dead_unit_data: (
                                1,
                                (
                                    (0, 0),
                                    (
                                        CompanyTypes::None,
                                        (-1, -1, -1, -1, -1, -1, -1),
                                        "".to_string(),
                                    ),
                                ),
                                None,
                                unit_entity,
                                *unit_transform,
                                false,
                            ),
                        });
                    }
                }
            } else {
                unit_component.stuck_count = 0;
                unit_component.last_position = unit_transform.translation;
            }
        } else {
            if let Some(destination) = unit_component.quantized_destination {
                let area_cost_multiplier: Option<&[f32]>;
                if let Some(_) = logistic_unit_option {
                    area_cost_multiplier = Some(&[10.0, 0.1]);
                } else {
                    area_cost_multiplier = Some(&[1.0, 1.5]);
                }

                let mut new_destination = destination;

                if unit_transform.translation.distance(destination) > QUANTIZATION_THRESHOLD {
                    new_destination = unit_transform.translation + (destination - unit_transform.translation).normalize() * QUANTIZATION_THRESHOLD;
                } else {
                    unit_component.quantized_destination = None;
                }

                unstarted_tasks.0.push((
                    TaskPoolTypes::Manual,
                    (
                        unit_transform.translation,
                        new_destination,
                        Some(100.),
                        unit_entity,
                    ),
                ));
            }

            commands.entity(unit_entity).remove::<NeedToMove>();
            commands.entity(unit_entity).try_insert(StoppedMoving);

            unit_component.elapsed = 0.;

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                        server_entity: unit_entity,
                        new_position: unit_transform.translation,
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    // for(mut unit_component,mut unit_transform, unit_entity, controller_option) in units_q.iter_mut(){
    //     if !unit_component.path.is_empty(){
    //         let next_point = unit_component.path[0];
    //         let mut direction = (next_point - unit_transform.translation).normalize();
    //         direction.y = 0.;

    //         let unit_position = unit_transform.translation;

    //         unit_transform.look_at(
    //             Vec3::new(
    //                 next_point.x,
    //                 unit_position.y,
    //                 next_point.z,
    //             ),
    //             Vec3::Y,
    //         );

    //         if let Some(mut controller) = controller_option {
    //             controller.translation = Some(direction * unit_component.speed * time.delta_seconds());
    //         } else {
    //             unit_transform.translation += direction * unit_component.speed * time.delta_seconds();
    //         }

    //         if unit_transform.translation.xz().distance(next_point.xz()) < unit_component.waypoint_radius {

    //             unit_component.path.remove(0);

    //             if unit_component.path.is_empty() {
    //                 commands.entity(unit_entity).remove::<NeedToMove>();
    //                 commands.entity(unit_entity).try_insert(StoppedMoving);

    //                 if matches!(network_status.0, NetworkStatuses::Host) {
    //                     let mut channel_id = 30;
    //                     while channel_id <= 59 {
    //                         if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
    //                             server_entity: unit_entity,
    //                             new_position: unit_transform.translation,
    //                         }){
    //                             channel_id += 1;
    //                         } else {
    //                             break;
    //                         }
    //                     }
    //                 }
    //             }
    //         }
    //     } else {
    //         commands.entity(unit_entity).remove::<NeedToMove>();
    //         commands.entity(unit_entity).try_insert(StoppedMoving);
    //     }
    // }
}

pub fn check_tiles(
    mut units_q: Query<(&Transform, &mut CombatComponent, Entity), With<NeedToMove>>,
    mut tile_map: ResMut<UnitsTileMap>,
    timer: ResMut<camera::TimerResource>,
){
    if timer.0.finished() {
        for (unit_transform, mut combat_component, unit_entity) in units_q.iter_mut() {
            if !combat_component.is_static {
                let x = (unit_transform.translation.x / TILE_SIZE) as i32;
                let z = (unit_transform.translation.z / TILE_SIZE) as i32;
        
                if combat_component.unit_data.0 != (x, z) {
                    tile_map.tiles.entry(combat_component.team).or_insert_with(HashMap::new).entry(combat_component.unit_data.0)
                    .or_insert_with(HashMap::new).remove(&unit_entity);
                    combat_component.unit_data.0 = (x, z);
                }
        
                tile_map.tiles.entry(combat_component.team).or_insert_with(HashMap::new).entry(combat_component.unit_data.0)
                .or_insert_with(HashMap::new).insert(unit_entity, (unit_transform.translation, combat_component.unit_type));
            }
        }
    }
}

pub fn find_targets(
    mut tile_map: ResMut<UnitsTileMap>,
    mut units_q: Query<(&Transform, &mut CombatComponent), With<CombatComponent>>,
    mut commands: Commands,
    timer: ResMut<camera::TimerResource>,
){
    if timer.0.finished() {
        let mut top_right_tile: (i32, i32);
        let mut bottom_left_tile: (i32, i32);
        let mut rows: i32;
        let mut columns: i32;
        let mut tile_to_scan: (i32, i32);
        let mut distance_to_target: f32;
        for (unit_transform, mut combat_component) in units_q.iter_mut() {
            if !matches!(combat_component.attack_type, AttackTypes::None) && combat_component.unit_data.0 != (i32::MAX, i32::MAX) {
                combat_component.enemies.clear();

                let damage_type;

                match combat_component.attack_type {
                    AttackTypes::Direct(_, _, damage_types) => {damage_type = damage_types},
                    AttackTypes::BallisticProjectile(_, _, _, _, _, direct_damage, splash_damage, _) => {damage_type = direct_damage.1},
                    AttackTypes::HomingProjectile(_, _, _, _, direct_damage, splash_damage, _) => {damage_type = direct_damage.1},
                    AttackTypes::None => {damage_type = DamageTypes::Universal},
                }

                let mut infantry: Vec<(Entity, f32)> = Vec::new();
                let mut light_vehicles: Vec<(Entity, f32)> = Vec::new();
                let mut heavy_vehicles: Vec<(Entity, f32)> = Vec::new();
                let mut buildings: Vec<(Entity, f32)> = Vec::new();

                top_right_tile = (
                    ((unit_transform.translation.x + combat_component.detection_range) / TILE_SIZE) as i32,
                    ((unit_transform.translation.z + combat_component.detection_range) / TILE_SIZE) as i32
                );
                bottom_left_tile = (
                    ((unit_transform.translation.x - combat_component.detection_range) / TILE_SIZE) as i32,
                    ((unit_transform.translation.z - combat_component.detection_range) / TILE_SIZE) as i32
                );
                tile_to_scan = bottom_left_tile;
                rows = top_right_tile.1 - bottom_left_tile.1;
                columns = top_right_tile.0 - bottom_left_tile.0;

                let mut enemy_count = 0;
                let mut is_enemy_count_overflow = false;

                let mut units_to_clear: Vec<(i32, (i32, i32), Entity)> = Vec::new();
                for _row in 0..rows + 1 {
                    for _column in 0..columns + 1 {
                        for team_tile_map in tile_map.tiles.iter_mut() {
                            if *team_tile_map.0 == combat_component.team {
                                continue;
                            }

                            for (unit_entity, (unit_position, unit_type)) in team_tile_map.1.entry(tile_to_scan)
                            .or_insert_with(HashMap::new) {
                                if commands.get_entity(*unit_entity).is_none() {
                                    units_to_clear.push((*team_tile_map.0, tile_to_scan, *unit_entity));

                                    continue;
                                }

                                distance_to_target = unit_transform.translation.xz().distance(unit_position.xz());

                                if distance_to_target <= combat_component.detection_range {
                                    match unit_type {
                                        UnitTypes::Infantry => {
                                            infantry.push((*unit_entity, distance_to_target));

                                            enemy_count += 1;
                                        },
                                        UnitTypes::LightVehicle => {
                                            light_vehicles.push((*unit_entity, distance_to_target));

                                            enemy_count += 1;
                                        },
                                        UnitTypes::HeavyVehicle => {
                                            heavy_vehicles.push((*unit_entity, distance_to_target));

                                            enemy_count += 1;
                                        },
                                        UnitTypes::Building =>  {
                                            buildings.push((*unit_entity, distance_to_target));

                                            enemy_count += 1;
                                        },
                                        UnitTypes::None => {},
                                    }

                                    if enemy_count > 10 {
                                        is_enemy_count_overflow = true;

                                        break;
                                    }
                                }
                            }
                        }

                        if is_enemy_count_overflow {
                            break;
                        }
        
                        tile_to_scan.0 += 1;
                    }

                    if is_enemy_count_overflow {
                        break;
                    }
        
                    tile_to_scan.1 += 1;
                    tile_to_scan.0 -= columns + 1;
                }

                for unit_to_clear in units_to_clear.iter() {
                    tile_map.tiles.entry(unit_to_clear.0).or_insert_with(HashMap::new).entry(unit_to_clear.1)
                    .or_insert_with(HashMap::new).remove(&unit_to_clear.2);
                }

                infantry.sort_by_key(|&(_enemy, distance)| distance as i32);
                light_vehicles.sort_by_key(|&(_enemy, distance)| distance as i32);
                heavy_vehicles.sort_by_key(|&(_enemy, distance)| distance as i32);
                buildings.sort_by_key(|&(_enemy, distance)| distance as i32);

                match damage_type {
                    DamageTypes::AntiInfantry => {
                        combat_component.enemies.append(&mut infantry);
                        combat_component.enemies.append(&mut light_vehicles);
                        combat_component.enemies.append(&mut heavy_vehicles);
                        combat_component.enemies.append(&mut buildings);
                    },
                    DamageTypes::AntiTank => {
                        combat_component.enemies.append(&mut heavy_vehicles);
                        combat_component.enemies.append(&mut light_vehicles);
                        combat_component.enemies.append(&mut buildings);
                        combat_component.enemies.append(&mut infantry);
                    },
                    DamageTypes::AntiBuilding => {
                        combat_component.enemies.append(&mut buildings);
                        combat_component.enemies.append(&mut light_vehicles);
                        combat_component.enemies.append(&mut heavy_vehicles);
                        combat_component.enemies.append(&mut infantry);
                    },
                    DamageTypes::Universal => {
                        combat_component.enemies.append(&mut infantry);
                        combat_component.enemies.append(&mut light_vehicles);
                        combat_component.enemies.append(&mut heavy_vehicles);
                        combat_component.enemies.append(&mut buildings);
                    },
                }
            }
        }
    }
}

pub fn process_combat (
    mut units_q: Query<(&mut CombatComponent, &mut Transform, Option<&Covered>, Entity, Option<&Children>, &GlobalTransform, Option<&SuppliesConsumerComponent>, Option<&CoverComponent>, Option<&InfantryTransport>),
    (With<CombatComponent>, Without<CameraComponent>, Without<DisabledUnit>)>,
    disabled_units_q: Query<(&CombatComponent, &mut Transform, Option<&Covered>, Entity, Option<&Children>, &GlobalTransform, Option<&SuppliesConsumerComponent>, Option<&CoverComponent>, Option<&InfantryTransport>),
    (With<CombatComponent>, Without<CameraComponent>, With<DisabledUnit>)>,
    mut transforms_q: Query<(&mut Transform, &GlobalTransform), (Without<CombatComponent>, Without<CameraComponent>)>,
    camera_q: Query<&Transform, (With<CameraComponent>, Without<CombatComponent>)>,
    mut event_writer:(
        //EventWriter<UnsentServerMessage>,
        EventWriter<UnitDeathEvent>,
    ),
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    instanced_materials: Res<InstancedMaterials>,
    attack_visualisation_assets: Res<AttackVisualisationAssets>,
    clients: Res<ClientList>,
){
    match network_status.0 {
        NetworkStatuses::Client => {
            let mut entities_to_rotate: Vec<(Entity, Vec3)> = Vec::new();
            let mut attacks_to_simulate: Vec<(Entity, Vec3)> = Vec::new();

            for unit in units_q.iter(){
                if !matches!(unit.0.attack_type, AttackTypes::None){
                    if unit.0.enemies.is_empty() {
                        if let Some(children) = unit.4 {
                            for child in children {
                                if let  Ok(mut transform) = transforms_q.get_mut(*child) {
                                    transform.0.rotation = Quat::IDENTITY;
                                }
                            }
                        }
                    } else {
                        if let Some(children) = unit.4 {
                            for enemy_entity in unit.0.enemies.iter() {
                                if let Ok(enemy) = units_q.get(enemy_entity.0) {
                                    if enemy.0.current_health > 0 && unit.1.translation.distance(enemy.1.translation) <= unit.0.attack_range {
                                        for child in children.iter() {
                                            if let  Ok(mut transform) = transforms_q.get_mut(*child) {
                                                let child_world_pos = transform.1.translation();
                                                let target_pos = enemy.1.translation;

                                                let mut direction = (target_pos - child_world_pos).normalize();
                                                direction.y = 0.;

                                                let desired_world_rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);

                                                let parent_global_transform = unit.5;
                                                let parent_world_rotation = parent_global_transform.compute_transform().rotation;

                                                let local_rotation = parent_world_rotation.inverse() * desired_world_rotation;

                                                transform.0.rotation = local_rotation;
                                            }
                                        }
                                        
                                        if let Some(supplies_consumer) = unit.6 {
                                            if supplies_consumer.supplies > 0 {
                                                attacks_to_simulate.push((unit.3, enemy.1.translation));
                                            }
                                        } else {
                                            attacks_to_simulate.push((unit.3, enemy.1.translation));
                                        }

                                        break;
                                    }
                                }
                            }
                        } else {
                            for enemy_entity in unit.0.enemies.iter() {
                                if let Ok(enemy) = units_q.get(enemy_entity.0) {
                                    if enemy.0.current_health > 0 && unit.1.translation.distance(enemy.1.translation) <= unit.0.attack_range {
                                        if let Some(supplies_consumer) = unit.6 {
                                            if supplies_consumer.supplies > 0 {
                                                attacks_to_simulate.push((unit.3, enemy.1.translation));
                                            }
                                        } else {
                                            attacks_to_simulate.push((unit.3, enemy.1.translation));
                                        }

                                        entities_to_rotate.push((unit.3, enemy.1.translation));

                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for entity in entities_to_rotate.iter() {
                if let Ok (mut unit) = units_q.get_mut(entity.0) {
                    let y = unit.1.translation.y;

                    unit.1.look_at(
                        Vec3::new(
                            entity.1.x,
                            y,
                            entity.1.z,
                        ), 
                        Vec3::Y,
                    );
                }
            }

            let camera_pos = camera_q.single().translation;

            let mut sounds_needs_to_play: HashMap<AttackAnimationTypes, Vec<(Entity, f32)>> = HashMap::new();

            for attack_attempt in attacks_to_simulate.iter() {
                if let Ok(mut unit) = units_q.get_mut(attack_attempt.0) {
                    unit.0.attack_elapsed_time += time.delta().as_millis();

                    if unit.0.attack_elapsed_time >= unit.0.attack_frequency {
                        unit.0.attack_elapsed_time = 0;
                    } else {
                        continue;
                    }

                    let mut rng = rand::thread_rng();

                    let displaced_enemy_pos = Vec3::new(
                        attack_attempt.1.x + rng.gen_range(-1.0..=1.0),
                        attack_attempt.1.y + rng.gen_range(-1.0..=1.0),
                        attack_attempt.1.z + rng.gen_range(-1.0..=1.0),
                    );

                    let attack_animation_type_clone = unit.0.attack_animation_type.clone();

                    match unit.0.attack_animation_type {
                        AttackAnimationTypes::LowCaliber(p) => {
                            if rng.gen_range(1..=3) != 3 {
                                commands.spawn(attack_visualisation_assets.bullet_low.0.clone())
                                .try_insert(Transform::from_translation(unit.1.translation + p).looking_at(displaced_enemy_pos, Vec3::Y))
                                .try_insert(NotShadowCaster)
                                .try_insert(BulletSprite{
                                    lifetime: 2000,
                                    elapsed_time: 0,
                                    speed: 50.,
                                    direction: (displaced_enemy_pos - unit.1.translation).normalize(),
                                });

                                sounds_needs_to_play.entry(unit.0.attack_animation_type.clone()).or_insert_with(Vec::new);

                                if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                    sounds.push((unit.3, camera_pos.distance(unit.1.translation + p)));
                                }
                            }
                        },
                        AttackAnimationTypes::HighCaliber(p) => {
                            commands.spawn(attack_visualisation_assets.bullet_high.0.clone())
                            .try_insert(Transform::from_translation(unit.1.translation + p).looking_at(displaced_enemy_pos, Vec3::Y))
                            .try_insert(NotShadowCaster)
                            .try_insert(BulletSprite{
                                lifetime: 2000,
                                elapsed_time: 0,
                                speed: 100.,
                                direction: (displaced_enemy_pos - unit.1.translation).normalize(),
                            });

                            sounds_needs_to_play.entry(unit.0.attack_animation_type.clone()).or_insert_with(Vec::new);

                            if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                sounds.push((unit.3, camera_pos.distance(unit.1.translation + p)));
                            }
                        },
                        AttackAnimationTypes::MissileLaunch(p) => {
                            sounds_needs_to_play.entry(unit.0.attack_animation_type.clone()).or_insert_with(Vec::new);

                            if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                sounds.push((unit.3, camera_pos.distance(unit.1.translation + p)));
                            }
                        },
                        AttackAnimationTypes::TankCannon(p) => {
                            sounds_needs_to_play.entry(unit.0.attack_animation_type.clone()).or_insert_with(Vec::new);

                            if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                sounds.push((unit.3, camera_pos.distance(unit.1.translation + p)));
                            }
                        },
                        AttackAnimationTypes::None(_p) => {},
                    }
                }
            }

            for sounds_type in sounds_needs_to_play.iter_mut() {
                let mut counter = 0;

                sounds_type.1.sort_by_key(|&(_source_entity, distance)| distance as i32);

                match sounds_type.0 {
                    AttackAnimationTypes::LowCaliber(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.bullet_low.1.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::HighCaliber(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.bullet_high.1.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::MissileLaunch(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.missile_launch_sound.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::TankCannon(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.tank_shot_sound.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::None(_) => {},
                }
            }
        }
        _ =>{
            let camera_pos = camera_q.single().translation;
            let mut entities_to_rotate: Vec<(Entity, Vec3)> = Vec::new();

            for unit in units_q.iter(){
                if !matches!(unit.0.attack_type, AttackTypes::None){
                    if unit.0.enemies.is_empty() {
                        if let Some(children) = unit.4 {
                            for child in children {
                                if let  Ok(mut transform) = transforms_q.get_mut(*child) {
                                    transform.0.rotation = Quat::IDENTITY;
                                }
                            }
                        }
                    } else {
                        if let Some(children) = unit.4 {
                            for enemy_entity in unit.0.enemies.iter() {
                                if let Ok(enemy) = units_q.get(enemy_entity.0) {
                                    if enemy.0.current_health > 0 && unit.1.translation.distance(enemy.1.translation) <= unit.0.attack_range {
                                        for child in children.iter() {
                                            if let  Ok(mut transform) = transforms_q.get_mut(*child) {
                                                let child_world_pos = transform.1.translation();
                                                let target_pos = enemy.1.translation;

                                                let mut direction = (target_pos - child_world_pos).normalize();
                                                direction.y = 0.;

                                                let desired_world_rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);

                                                let parent_global_transform = unit.5;
                                                let parent_world_rotation = parent_global_transform.compute_transform().rotation;

                                                let local_rotation = parent_world_rotation.inverse() * desired_world_rotation;

                                                transform.0.rotation = local_rotation;
                                            }
                                        }

                                        break;
                                    }
                                }
                            }
                        } else {
                            for enemy_entity in unit.0.enemies.iter() {
                                if let Ok(enemy) = units_q.get(enemy_entity.0) {
                                    if enemy.0.current_health > 0 && unit.1.translation.distance(enemy.1.translation) <= unit.0.attack_range {
                                        entities_to_rotate.push((unit.3, enemy.1.translation));

                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for entity in entities_to_rotate.iter() {
                if let Ok (mut unit) = units_q.get_mut(entity.0) {
                    let y = unit.1.translation.y;

                    unit.1.look_at(
                        Vec3::new(
                            entity.1.x,
                            y,
                            entity.1.z,
                        ), 
                        Vec3::Y,
                    );
                }
            }

            let mut attacker_entities: Vec<Entity> = Vec::new();

            for mut unit in units_q.iter_mut(){
                if let Some(consumer) = unit.6 {
                    if consumer.supplies <= 0 {continue;}
                }

                if !matches!(unit.0.attack_type, AttackTypes::None){
                    unit.0.attack_elapsed_time += time.delta().as_millis();

                    if unit.0.attack_elapsed_time >= unit.0.attack_frequency {
                        unit.0.attack_elapsed_time = 0;

                        attacker_entities.push(unit.3);
                    }
                }
            }

            let mut sounds_needs_to_play: HashMap<AttackAnimationTypes, Vec<(Entity, f32)>> = HashMap::new();

            let mut extra_units_to_kill: Vec<Entity> = Vec::new();

            for attacker_entity in attacker_entities.iter() {
                let mut enemies: Vec<(Entity, f32)> = Vec::new();
                let mut range: f32 = 0.;
                let mut attacker_pos: Vec3 = Vec3::ZERO;
                let mut attack_type = AttackTypes::None;
                let mut attack_animation_type = AttackAnimationTypes::None(Vec3::ZERO);

                if let Ok(attacker) = units_q.get(*attacker_entity) {
                    if attacker.0.current_health > 0 {
                        enemies = attacker.0.enemies.clone();
                        range = attacker.0.attack_range;
                        attacker_pos = attacker.1.translation;
                        attack_type = attacker.0.attack_type.clone();
                        attack_animation_type = attacker.0.attack_animation_type.clone();
                    } else {
                        continue;
                    }
                }

                for enemy_entity in enemies.iter() {
                    if let Ok(mut enemy) = units_q.get_mut(enemy_entity.0) {
                        if enemy.0.current_health > 0 && attacker_pos.distance(enemy.1.translation) <= range {
                            match attack_type {
                                AttackTypes::Direct(damage, accuracy, damage_type) => {
                                    let mut rng = rand::thread_rng();

                                    let mut cover_efficiency = 1.;
                                    let mut cover_entity = None;
                                    if let Some(cover) = enemy.2 {
                                        cover_efficiency = cover.cover_efficiency;
                                        cover_entity = Some(cover.cover_entity);
                                    }
                        
                                    if accuracy.clone() >= rng.gen_range(0.0..1.0) * cover_efficiency {
                                        let mut current_damage = damage.clone();
                    
                                        match damage_type {
                                            DamageTypes::AntiInfantry => {
                                                match enemy.0.unit_type {
                                                    UnitTypes::Infantry => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::LightVehicle => {
                                                        current_damage /= 5;
                                                    },
                                                    UnitTypes::HeavyVehicle => {
                                                        current_damage /= 10;
                                                    },
                                                    UnitTypes::Building => {
                                                        current_damage /= 10;
                                                    },
                                                    UnitTypes::None => {},
                                                }
                                            },
                                            DamageTypes::AntiTank => {
                                                match enemy.0.unit_type {
                                                    UnitTypes::Infantry => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::LightVehicle => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::HeavyVehicle => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::Building => {
                                                        current_damage /= 2;
                                                    },
                                                    UnitTypes::None => {},
                                                }
                                            },
                                            DamageTypes::AntiBuilding => {
                                                match enemy.0.unit_type {
                                                    UnitTypes::Infantry => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::LightVehicle => {
                                                        current_damage /= 2;
                                                    },
                                                    UnitTypes::HeavyVehicle => {
                                                        current_damage /= 3;
                                                    },
                                                    UnitTypes::Building => {
                                                        current_damage *= 1;
                                                    },
                                                    UnitTypes::None => {},
                                                }
                                            },
                                            DamageTypes::Universal => {},
                                        }
                    
                                        enemy.0.current_health -= current_damage;
                    
                                        if matches!(network_status.0, NetworkStatuses::Host) {
                                            let mut channel_id = 00;
                                            while channel_id <= 59 {
                                                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitDamaged {
                                                    server_entity: enemy.3,
                                                    damage: current_damage,
                                                }){
                                                    channel_id += 1;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                    
                                        if enemy.0.current_health <= 0 {
                                            event_writer.0.send(UnitDeathEvent { dead_unit_data:
                                                (
                                                    enemy.0.team,
                                                    enemy.0.unit_data.clone(),
                                                    cover_entity,
                                                    enemy.3,
                                                    *enemy.1,
                                                    true,
                                                )
                                            });

                                            if let Some(cover) = enemy.7 {
                                                for unit in cover.units_inside.iter() {
                                                    extra_units_to_kill.push(*unit);
                                                }
                                            }

                                            if let Some(transport) = enemy.8 {
                                                for unit in transport.units_inside.iter() {
                                                    extra_units_to_kill.push(*unit);
                                                }
                                            }
                                        }
                                    }
                                },
                                AttackTypes::BallisticProjectile
                                (max_height, points_num, speed, check_factor, accuracy, direct_damage, splash_damage, spawn_pos) => {
                                    let mut rng = rand::thread_rng();

                                    let mut end_point = enemy.1.translation;
                                    if accuracy != 0. {
                                        end_point.x += rng.gen_range(-accuracy..accuracy);
                                        end_point.z += rng.gen_range(-accuracy..accuracy);
                                    }
                    
                                    let shell_entity;
                                    if points_num > 2 {
                                        shell_entity = commands.spawn(MaterialMeshBundle {
                                            mesh: attack_visualisation_assets.shell.0.clone(),
                                            material: attack_visualisation_assets.shell.1.clone(),
                                            ..default()
                                        })
                                        .try_insert(Transform::from_translation(attacker_pos + spawn_pos).looking_at(end_point, Vec3::Y))
                                        .try_insert(TrailEmmiterComponent)
                                        .try_insert(BallisticProjectile{
                                            path: generate_parabolic_trajectory(
                                                attacker_pos,
                                                end_point,
                                                max_height,
                                                points_num),
                                            speed: speed,
                                            start_position: attacker_pos,
                                            target_position: end_point,
                                            waypoint_radius: check_factor,
                                            elapsed: 0.,
                                            inv_duration: 0.,
                                            direct_damage: direct_damage,
                                            splash_damage: splash_damage,
                                        }).id();
                                    } else {
                                        shell_entity = commands.spawn(MaterialMeshBundle {
                                            mesh: attack_visualisation_assets.shell.0.clone(),
                                            material: attack_visualisation_assets.shell.1.clone(),
                                            ..default()
                                        })
                                        .try_insert(Transform::from_translation(attacker_pos + spawn_pos).looking_at(end_point, Vec3::Y))
                                        .try_insert(TrailEmmiterComponent)
                                        .try_insert(BallisticProjectile{
                                            path: vec![end_point + Vec3::new(0., 1., 0.)],
                                            speed: speed,
                                            start_position: attacker_pos,
                                            target_position: end_point,
                                            waypoint_radius: check_factor,
                                            elapsed: 0.,
                                            inv_duration: 0.,
                                            direct_damage: direct_damage,
                                            splash_damage: splash_damage,
                                        }).id();
                                    }

                                    let mesh_handle = meshes.add(Triangle3d{
                                        vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
                                    });

                                    commands.spawn(MaterialMeshBundle{
                                        mesh: mesh_handle.clone(),
                                        material: instanced_materials.red_solid.clone(),
                                        transform: Transform::from_translation(attacker_pos + spawn_pos),
                                        ..default()
                                    })
                                    .try_insert(
                                        TrailComponent{
                                            positions: vec![],
                                            length: 10,
                                            width: 0.05,
                                            mesh_handle,
                                            emmiter_entity: shell_entity,
                                        }
                                    );
                    
                                    if matches!(network_status.0, NetworkStatuses::Host) {
                                        let mut channel_id = 60;
                                        while channel_id <= 89 {
                                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ArtilleryProjectileSpawned {
                                                position: attacker_pos,
                                                server_entity: shell_entity,
                                            }){
                                                channel_id += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                },
                                AttackTypes::HomingProjectile
                                (speed, waypoint_check_factor, max_prediction_iterations, prediction_tolerance, direct_damage, splash_damage, spawn_pos) => {
                                    
                                    let projectile = commands.spawn(MaterialMeshBundle{
                                        mesh: attack_visualisation_assets.shell.0.clone(),
                                        material: attack_visualisation_assets.shell.1.clone(),
                                        ..default()
                                    })
                                    .try_insert(Transform::from_translation(attacker_pos + spawn_pos).looking_at(enemy.1.translation, Vec3::Y))
                                    .try_insert(HomingProjectile{
                                        speed: speed,
                                        hit_check_factor: waypoint_check_factor,
                                        target_entity: enemy.3,
                                        targets_last_position: enemy.1.translation,
                                        max_prediction_caluclation_iterations: max_prediction_iterations,
                                        prediction_tolerance: prediction_tolerance,
                                        direct_damage: direct_damage,
                                        splash_damage: splash_damage,
                                    })
                                    .try_insert(TrailEmmiterComponent)
                                    .id();

                                    let mesh_handle = meshes.add(Triangle3d{
                                        vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
                                    });

                                    commands.spawn(MaterialMeshBundle{
                                        mesh: mesh_handle.clone(),
                                        material: instanced_materials.red_solid.clone(),
                                        transform: Transform::from_translation(attacker_pos + spawn_pos),
                                        ..default()
                                    })
                                    .try_insert(
                                        TrailComponent{
                                            positions: vec![],
                                            length: 10,
                                            width: 0.05,
                                            mesh_handle,
                                            emmiter_entity: projectile,
                                        }
                                    );
                    
                                    if matches!(network_status.0, NetworkStatuses::Host) {
                                        let mut channel_id = 60;
                                        while channel_id <= 89 {
                                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::HomingProjectileSpawned {
                                                position: attacker_pos,
                                                server_entity: projectile,
                                            }){
                                                channel_id += 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                },
                                AttackTypes::None => {},
                            }

                            let mut rng = rand::thread_rng();

                            let displaced_enemy_pos = Vec3::new(
                                enemy.1.translation.x + rng.gen_range(-1.0..=1.0),
                                enemy.1.translation.y + rng.gen_range(-1.0..=1.0),
                                enemy.1.translation.z + rng.gen_range(-1.0..=1.0),
                            );

                            let attack_animation_type_clone = attack_animation_type.clone();

                            match attack_animation_type {
                                AttackAnimationTypes::LowCaliber(p) => {
                                    if rng.gen_range(1..=3) != 3 {
                                        commands.spawn(attack_visualisation_assets.bullet_low.0.clone())
                                        .try_insert(Transform::from_translation(attacker_pos + p).looking_at(displaced_enemy_pos, Vec3::Y))
                                        .try_insert(NotShadowCaster)
                                        .try_insert(BulletSprite{
                                            lifetime: 2000,
                                            elapsed_time: 0,
                                            speed: 50.,
                                            direction: (displaced_enemy_pos - attacker_pos).normalize(),
                                        });

                                        sounds_needs_to_play.entry(attack_animation_type).or_insert_with(Vec::new);

                                        if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                            sounds.push((*attacker_entity, camera_pos.distance(attacker_pos + p)));
                                        }
                                    }
                                },
                                AttackAnimationTypes::HighCaliber(p) => {
                                    commands.spawn(attack_visualisation_assets.bullet_high.0.clone())
                                    .try_insert(Transform::from_translation(attacker_pos + p).looking_at(displaced_enemy_pos, Vec3::Y))
                                    .try_insert(NotShadowCaster)
                                    .try_insert(BulletSprite{
                                        lifetime: 2000,
                                        elapsed_time: 0,
                                        speed: 100.,
                                        direction: (displaced_enemy_pos - attacker_pos).normalize(),
                                    });

                                    sounds_needs_to_play.entry(attack_animation_type).or_insert_with(Vec::new);

                                    if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                        sounds.push((*attacker_entity, camera_pos.distance(attacker_pos + p)));
                                    }
                                },
                                AttackAnimationTypes::MissileLaunch(p) => {
                                    sounds_needs_to_play.entry(attack_animation_type).or_insert_with(Vec::new);

                                    if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                        sounds.push((*attacker_entity, camera_pos.distance(attacker_pos + p)));
                                    }
                                },
                                AttackAnimationTypes::TankCannon(p) => {
                                    sounds_needs_to_play.entry(attack_animation_type).or_insert_with(Vec::new);

                                    if let Some(sounds) = sounds_needs_to_play.get_mut(&attack_animation_type_clone) {
                                        sounds.push((*attacker_entity, camera_pos.distance(attacker_pos + p)));
                                    }
                                },
                                AttackAnimationTypes::None(_p) => {},
                            }

                            break;
                        }
                    }
                }
            }

            for sounds_type in sounds_needs_to_play.iter_mut() {
                let mut counter = 0;

                sounds_type.1.sort_by_key(|&(_source_entity, distance)| distance as i32);

                match sounds_type.0 {
                    AttackAnimationTypes::LowCaliber(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.bullet_low.1.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::HighCaliber(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.bullet_high.1.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::MissileLaunch(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.missile_launch_sound.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::TankCannon(_) => {
                        for source_entity in sounds_type.1.iter() {
                            if commands.get_entity(source_entity.0).is_some() {
                                counter += 1;

                                commands.entity(source_entity.0).remove::<AudioBundle>();

                                commands.entity(source_entity.0).try_insert(
                                    AudioBundle{
                                        source: attack_visualisation_assets.tank_shot_sound.clone(),
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

                                if counter >= 5 {break;}
                            }
                        }
                    },
                    AttackAnimationTypes::None(_) => {},
                }
            }

            for extra_unit_to_kill in extra_units_to_kill.iter() {
                if let Ok(unit) = units_q.get(*extra_unit_to_kill) {
                    let mut cover_entity: Option<Entity> = None;

                    if let Some(cover) = unit.2 {
                        cover_entity = Some(cover.cover_entity);
                    }

                    event_writer.0.send(UnitDeathEvent { dead_unit_data:
                        (
                            unit.0.team,
                            unit.0.unit_data.clone(),
                            cover_entity,
                            unit.3,
                            *unit.1,
                            true,
                        )
                    });
                } else if let Ok(unit) = disabled_units_q.get(*extra_unit_to_kill) {
                    let mut cover_entity: Option<Entity> = None;

                    if let Some(cover) = unit.2 {
                        cover_entity = Some(cover.cover_entity);
                    }

                    event_writer.0.send(UnitDeathEvent { dead_unit_data:
                        (
                            unit.0.team,
                            unit.0.unit_data.clone(),
                            cover_entity,
                            unit.3,
                            *unit.1,
                            true,
                        )
                    });
                }
            }
        }
    }
}

pub fn platoon_leaders_monitoring_system (
    mut army: ResMut<Armies>,
    mut commands: Commands,
    timer: ResMut<camera::TimerResource>,
){
    if timer.0.finished() {
        for team_army in army.0.iter_mut() {
            for platoon in team_army.1.regular_squads.iter_mut() {
                if platoon.1.2 == Entity::PLACEHOLDER || commands.get_entity(platoon.1.2).is_none() {
                    let mut soldiers_iter = platoon.1.0.0.0.set.iter();
                    let mut specialists_iter = platoon.1.0.0.1.set.iter();

                    loop {
                        if let Some(single_unit) = soldiers_iter.next() {
                            if commands.get_entity(*single_unit).is_some() {
                                platoon.1.2 = *single_unit;
                                commands.entity(platoon.1.2).try_insert(SquadLeader((
                                    CompanyTypes::Regular,
                                    *platoon.0
                                )));

                                break;
                            }
                        } else if let Some(single_unit) = specialists_iter.next() {
                            if commands.get_entity(*single_unit).is_some() {
                                platoon.1.2 = *single_unit;
                                commands.entity(platoon.1.2).try_insert(SquadLeader((
                                    CompanyTypes::Regular,
                                    *platoon.0
                                )));

                                break;
                            }
                        } else if platoon.1.2 != Entity::PLACEHOLDER {
                            platoon.1.2 = Entity::PLACEHOLDER;

                            break;
                        } else {
                            break;
                        }
                    }
                }
            }

            for platoon in team_army.1.shock_squads.iter_mut() {
                if platoon.1.2 == Entity::PLACEHOLDER || commands.get_entity(platoon.1.2).is_none() {
                    let mut soldiers_iter = platoon.1.0.0.0.set.iter();
                    let mut specialists_iter = platoon.1.0.0.0.set.iter();

                    loop {
                        if let Some(single_unit) = soldiers_iter.next() {
                            if commands.get_entity(*single_unit).is_some() {
                                platoon.1.2 = *single_unit;
                                commands.entity(platoon.1.2).try_insert(SquadLeader((
                                    CompanyTypes::Shock,
                                    *platoon.0
                                )));

                                break;
                            }
                        } else if let Some(single_unit) = specialists_iter.next() {
                            if commands.get_entity(*single_unit).is_some() {
                                platoon.1.2 = *single_unit;
                                commands.entity(platoon.1.2).try_insert(SquadLeader((
                                    CompanyTypes::Shock,
                                    *platoon.0
                                )));

                                break;
                            }
                        } else if platoon.1.2 != Entity::PLACEHOLDER {
                            platoon.1.2 = Entity::PLACEHOLDER;

                            break;
                        } else {
                            break;
                        }
                    }
                }
            }

            for platoon in team_army.1.armored_squads.iter_mut() {
                if platoon.1.2 == Entity::PLACEHOLDER || commands.get_entity(platoon.1.2).is_none() {
                    let mut vehicles_iter = platoon.1.0.0.set.iter();

                    loop {
                        if let Some(single_unit) = vehicles_iter.next() {
                            if commands.get_entity(*single_unit).is_some() {
                                platoon.1.2 = *single_unit;
                                commands.entity(platoon.1.2).try_insert(SquadLeader((
                                    CompanyTypes::Armored,
                                    *platoon.0
                                )));

                                break;
                            }
                        } else if platoon.1.2 != Entity::PLACEHOLDER {
                            platoon.1.2 = Entity::PLACEHOLDER;

                            break;
                        } else {
                            break;
                        }
                    }
                }
            }
            
            // let mut artillery_battalion_leader = army.0.get_mut(&player_data.team).unwrap().artillery_units.1;
            // if artillery_battalion_leader == Entity::PLACEHOLDER ||
            // commands.get_entity(artillery_battalion_leader).is_none() {
            //     for artillery_unit in army.0.get_mut(&player_data.team).unwrap().artillery_units.0.iter_mut() {
            //         if let Some(artillery_entity) = artillery_unit.1.0.0 {
            //             if commands.get_entity(artillery_entity).is_some() {
            //                 artillery_battalion_leader = artillery_entity;

            //                 commands.entity(artillery_battalion_leader).try_insert(PlatoonLeader((
            //                     BattalionTypes::Artillery,
            //                     (0, 0, 0, 0, 0),
            //                 )));

            //                 break;
            //             }
            //         }
            //     }
            // }
        }
    }
}

pub fn squad_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<SquadSelectionEvent>,
){
    for event in event_reader.read() {
        match  event.0.0 {
            CompanyTypes::Regular => {
                if let Some(platoon) = army.0.get(&player_data.team).unwrap().regular_squads.get(&event.0.1.clone()){
                    let mut units: Vec<Entity> = platoon.0.0.0.set.iter().cloned().collect();
                    let mut specialists: Vec<Entity> = platoon.0.0.1.set.iter().cloned().collect();
                    units.append(&mut specialists);

                    clear_selected_units(&mut selected_units, &mut commands, &units_q);

                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Shock => {
                if let Some(platoon) = army.0.get(&player_data.team).unwrap().shock_squads.get(&event.0.1.clone()){
                    let mut units: Vec<Entity> = platoon.0.0.0.set.iter().cloned().collect();
                    let mut specialists: Vec<Entity> = platoon.0.0.1.set.iter().cloned().collect();
                    units.append(&mut specialists);

                    clear_selected_units(&mut selected_units, &mut commands, &units_q);

                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Armored => {
                if let Some(platoon) = army.0.get(&player_data.team).unwrap().armored_squads.get(&event.0.1.clone()){
                    let units: Vec<Entity> = platoon.0.0.set.iter().cloned().collect();

                    clear_selected_units(&mut selected_units, &mut commands, &units_q);

                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            _ => {},
        }
    }
}

pub fn platoon_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<PlatoonSelectionEvent>,
){
    for event in event_reader.read() {
        match  event.0.0 {
            CompanyTypes::Regular => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().regular_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Shock => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().shock_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Armored => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().armored_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            _ => {},
        }
    }
}

pub fn company_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<CompanySelectionEvent>,
){
    for event in event_reader.read() {
        match  event.0.0 {
            CompanyTypes::Regular => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().regular_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Shock => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().shock_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            CompanyTypes::Armored => {
                let mut units: Vec<Entity> = Vec::new();

                for squad_id in event.0.1.iter() {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().armored_squads.get(squad_id){
                        let mut regular_units: Vec<Entity> = squad.0.0.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                    }
                }

                if units.len() > 0 {
                    clear_selected_units(&mut selected_units, &mut commands, &units_q);
                    add_selected_units(units, &mut selected_units, &mut commands, &units_q);
                }
            },
            _ => {},
        }
    }
}

pub fn battalion_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<BattalionSelectionEvent>,
){
    for event in event_reader.read() {
        let mut units: Vec<Entity> = Vec::new();

        for squad_id in event.0.iter() {
            match  squad_id.0 {
                CompanyTypes::Regular => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().regular_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Shock => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().shock_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Armored => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().armored_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                    }
                },
                _ => {},
            }
        }

        if units.len() > 0 {
            clear_selected_units(&mut selected_units, &mut commands, &units_q);
            add_selected_units(units, &mut selected_units, &mut commands, &units_q);
        }
    }
}

pub fn regiment_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<RegimentSelectionEvent>,
){
    for event in event_reader.read() {
        let mut units: Vec<Entity> = Vec::new();

        for squad_id in event.0.iter() {
            match  squad_id.0 {
                CompanyTypes::Regular => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().regular_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Shock => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().shock_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Armored => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().armored_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                    }
                },
                _ => {},
            }
        }

        if units.len() > 0 {
            clear_selected_units(&mut selected_units, &mut commands, &units_q);
            add_selected_units(units, &mut selected_units, &mut commands, &units_q);
        }
    }
}

pub fn brigade_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<BrigadeSelectionEvent>,
){
    for event in event_reader.read() {
        let mut units: Vec<Entity> = Vec::new();

        for squad_id in event.0.iter() {
            match  squad_id.0 {
                CompanyTypes::Regular => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().regular_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Shock => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().shock_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.0.set.iter().cloned().collect();
                        let mut specialists: Vec<Entity> = squad.0.0.1.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                        units.append(&mut specialists);
                    }
                },
                CompanyTypes::Armored => {
                    if let Some(squad) = army.0.get(&player_data.team).unwrap().armored_squads.get(&squad_id.1){
                        let mut regular_units: Vec<Entity> = squad.0.0.set.iter().cloned().collect();
                        units.append(&mut regular_units);
                    }
                },
                _ => {},
            }
        }

        if units.len() > 0 {
            clear_selected_units(&mut selected_units, &mut commands, &units_q);
            add_selected_units(units, &mut selected_units, &mut commands, &units_q);
        }
    }
}

pub fn artillery_unit_selection_system (
    army: Res<Armies>,
    mut selected_units: ResMut<SelectedUnits>,
    mut commands: Commands,
    units_q: Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
    mut event_reader: EventReader<ArtilleryUnitSelectedEvent>,
){
    for event in event_reader.read() {
        let mut units: Vec<Entity> = Vec::new();

        if let Some(team_army) = army.0.get(&event.0.0) {
            if let Some(artillery_unit_army_reference) = team_army.artillery_units.0.get(&event.0.1) {
                if let Some(artillery_unit_entity) = artillery_unit_army_reference.0.0 {
                    units.push(artillery_unit_entity);
                }
            }
        }

        if units.len() > 0 {
            clear_selected_units(&mut selected_units, &mut commands, &units_q);
            add_selected_units(units, &mut selected_units, &mut commands, &units_q);
        }
    }
}

pub fn cover_assignation_system (
    selected_units_q: Query<(Entity, &CombatComponent, &Transform), With<SelectedUnit>>,
    covers_q: Query<(Entity, &mut CoverComponent, &Transform, &CombatComponent), With<CoverComponent>>,
    player_data: Res<PlayerData>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    if mouse_buttons.just_released(MouseButton::Right){
        if !selected_units_q.is_empty() && !covers_q.is_empty() {
            if let Some(cursor_ray) = **cursor_ray {
                let hits = raycast.cast_ray(cursor_ray, &default());
        
                if hits.len() > 0 {
                    if let Ok(cover) = covers_q.get(hits[0].0) {
                        if cover.3.team != player_data.team || cover.1.points.len() <= cover.1.units_inside.len() {
                            return;
                        }

                        let mut selected_units_iter = selected_units_q.iter();

                        match network_status.0 {
                            NetworkStatuses::Client => {
                                let mut units_to_cover: Vec<Entity> = Vec::new();
                                let mut cover_entity = Entity::PLACEHOLDER;

                                if let Some(server_cover_entity) = entity_maps.client_to_server.get(&cover.0) {
                                    cover_entity = *server_cover_entity;
                                }
                                
                                for _i in 0..cover.1.points.len() - cover.1.units_inside.len() {
                                    if let Some(unit) = selected_units_iter.next() {
                                        if unit.1.unit_data.1.0 != CompanyTypes::Armored {
                                            if let Some(server_unit_entity) = entity_maps.client_to_server.get(&unit.0) {
                                                units_to_cover.push(*server_unit_entity);

                                                unstarted_tasks.0.push((
                                                    TaskPoolTypes::Manual,
                                                    (
                                                        unit.2.translation,
                                                        cover.2.translation,
                                                        Some(100.),
                                                        unit.0,
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                    else {
                                        break;
                                    }
                                }

                                if !units_to_cover.is_empty() && cover_entity != Entity::PLACEHOLDER {
                                    let mut channel_id = 30;
                                    while channel_id <= 59 {
                                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::CoverAssignationRequest{
                                            units: units_to_cover.clone(),
                                            cover_entity: cover_entity,
                                            cover_position: cover.2.translation,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            },
                            _ => {
                                for _i in 0..cover.1.points.len() - cover.1.units_inside.len() {
                                    if let Some(unit) = selected_units_iter.next() {
                                        if unit.1.unit_data.1.0 != CompanyTypes::Armored {
                                            commands.entity(unit.0).try_insert(MovingToCover{
                                                cover_entity: cover.0,
                                                cover_position: cover.2.translation,
                                            });

                                            unstarted_tasks.0.push((
                                                TaskPoolTypes::Manual,
                                                (
                                                    unit.2.translation,
                                                    cover.2.translation,
                                                    Some(100.),
                                                    unit.0,
                                                ),
                                            ));
                                        }
                                    }
                                    else {
                                        break;
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

pub fn unit_covering_system (
    mut moving_to_cover_q: Query<(Entity, &mut Transform, &MovingToCover, &mut UnitComponent), With<MovingToCover>>,
    mut covers_q: Query<(Entity, &mut CoverComponent, &Transform), (With<CoverComponent>, Without<MovingToCover>)>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for mut unit in moving_to_cover_q.iter_mut() {
        if unit.1.translation.distance(unit.2.cover_position) < 20. {
            if let Ok(mut cover) = covers_q.get_mut(unit.2.cover_entity) {
                if cover.1.points.len() > cover.1.units_inside.len() {
                    unit.3.path = Vec::new();
                    let original_y = unit.1.translation.y;
                    unit.1.translation = cover.1.points[cover.1.units_inside.len()] + cover.2.translation;

                    cover.1.units_inside.insert(unit.0);
    
                    commands.entity(unit.0).remove::<MovingToCover>();
                    commands.entity(unit.0).try_insert(Covered{
                        cover_efficiency: cover.1.cover_efficiency,
                        cover_entity: cover.0,
                        original_y: original_y,
                    });

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                                server_entity: unit.0,
                                new_position: cover.1.points[cover.1.units_inside.len() - 1] + cover.2.translation,
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }

                        channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitCovered{
                                server_entity: unit.0,
                                initial_unit_position_y: original_y,
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
}

pub fn cover_disturb_system (
    mut commands: Commands,
    mut units_q: Query<(Entity, Option<&Covered>, &mut Transform), With<SelectedUnit>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut event_writer: EventWriter<UnitNeedsToBeUncovered>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    if mouse_buttons.just_pressed(MouseButton::Right) {
        match network_status.0 {
            NetworkStatuses::Client => {
                let mut server_entities: Vec<Entity> = Vec::new();
                for unit in units_q.iter() {
                    if let Some(server_entity) = entity_maps.client_to_server.get(&unit.0) {
                        server_entities.push(*server_entity);
                    }
                }

                if !server_entities.is_empty() {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::UncoveringRequest{
                            unit_entities: server_entities.clone(),
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            },
            _ => {
                for mut unit in units_q.iter_mut() {
                    commands.entity(unit.0).remove::<MovingToCover>();
            
                    if let Some(cover) = unit.1 {
                        event_writer.send(UnitNeedsToBeUncovered{
                            cover_entity: cover.cover_entity,
                            unit_entity: unit.0,
                        });

                        unit.2.translation.y = cover.original_y;
                        commands.entity(unit.0).remove::<Covered>();
                    }
                }
            },
        }
    }
}

pub fn engineer_to_blueprint_assignation_system (
    mut unactivated_blueprints: ResMut<UnactivatedBlueprints>,
    mut construction_sites_q: Query<(Entity, &mut BuildingConstructionSite, &Transform),
    (With<BuildingConstructionSite>, Without<EngineerComponent>, Without<ToDeconstruct>, Without<DontTouch>)>,
    mut deconstruction_sites_q: Query<(Entity, &mut ToDeconstruct, &Transform, Option<&BuildingConstructionSite>),
    (With<ToDeconstruct>, Without<EngineerComponent>)>,
    free_engineers: Query<(Entity, &Transform, &CombatComponent),
    (With<EngineerComponent>, Without<BusyEngineer>, Without<BuildingConstructionSite>, Without<ToDeconstruct>)>,
    busy_engineers: Query<(Entity, &Transform, &CombatComponent),
    (With<EngineerComponent>, With<BusyEngineer>, Without<BuildingConstructionSite>, Without<ToDeconstruct>)>,
    mut commands: Commands,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    timer: ResMut<camera::TimerResource>,
    game_stage: Res<GameStage>,
){
    if timer.0.finished() {
        if let GameStages::GameStarted = game_stage.0 {
            let mut team_blueprints_to_delete: HashMap<i32, Vec<Entity>> = HashMap::new();

            for blueprints in unactivated_blueprints.0.iter_mut() {
                team_blueprints_to_delete.entry(*blueprints.0).or_insert_with(|| Vec::new());

                if let Some(blueprints_to_delete) = team_blueprints_to_delete.get_mut(blueprints.0) {
                    for blueprint in blueprints.1.iter() {
                        if commands.get_entity(*blueprint.0).is_none() {
                            blueprints_to_delete.push(*blueprint.0);
                        }
                    }
                }
            }

            for blueprints_to_delete in team_blueprints_to_delete.iter() {
                if let Some(unactivated_blueprints) = unactivated_blueprints.0.get_mut(blueprints_to_delete.0) {
                    for blueprint in blueprints_to_delete.1.iter() {
                        unactivated_blueprints.remove(blueprint);
                    }
                }
            }

            let mut team_blueprint_iters: HashMap<i32, IterMut<'_, Entity, (Vec3, Entity, f32)>> = HashMap::new();
            let mut is_all_blueprints_empty = true;

            for blueprints in unactivated_blueprints.0.iter_mut() {
                if !blueprints.1.is_empty() {
                    team_blueprint_iters.entry(*blueprints.0).or_insert_with(|| blueprints.1.iter_mut());
                    is_all_blueprints_empty = false;
                }
            }
            
            let mut free_engineers_iter = free_engineers.iter();

            if !is_all_blueprints_empty {
                loop {
                    if let Some(engineer) = free_engineers_iter.next(){
                        is_all_blueprints_empty = true;

                        for blueprint in team_blueprint_iters.iter(){
                            if blueprint.1.len() > 0 {
                                is_all_blueprints_empty = false;
                            }
                        }
    
                        if is_all_blueprints_empty {
                            break;
                        }
    
                        if let Some(blueprints) = team_blueprint_iters.get_mut(&engineer.2.team){
                            if let Some(blueprint) = blueprints.next(){
                                if commands.get_entity(blueprint.1.1).is_none() {
                                    blueprint.1.1 = engineer.0;
                                    commands.entity(engineer.0).try_insert(
                                        BusyEngineer(EngineerActions::ActivateBlueprint((blueprint.1.0, *blueprint.0, blueprint.1.2)))
                                    );

                                    unstarted_tasks.0.push((
                                        TaskPoolTypes::Extra,
                                        (
                                            engineer.1.translation,
                                            blueprint.1.0,
                                            Some(100.),
                                            engineer.0,
                                        ),
                                    ));
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
            }

            let mut team_construction_sites: HashMap<i32, Vec<(Entity, Mut<'_, BuildingConstructionSite>, &Transform)>> = HashMap::new();
            let mut team_construction_sites_iters: HashMap<i32, std::slice::IterMut<'_, (Entity, Mut<'_, BuildingConstructionSite>, &Transform)>> = HashMap::new();
            for construction_site in construction_sites_q.iter_mut() {
                team_construction_sites.entry(construction_site.1.team).or_insert_with(Vec::new).push(construction_site);
            }

            let mut is_all_construction_sites_empty = true;
            for construction_sites in team_construction_sites.iter_mut() {
                if !construction_sites.1.is_empty(){
                    is_all_construction_sites_empty = false;
                    team_construction_sites_iters.entry(*construction_sites.0).or_insert_with(|| construction_sites.1.iter_mut());
                }
            }

            if !is_all_construction_sites_empty {
                loop {
                    if let Some(engineer) = free_engineers_iter.next(){
                        is_all_construction_sites_empty = true;

                        for construction_sites in team_construction_sites_iters.iter() {
                            if construction_sites.1.len() > 0 {
                                is_all_construction_sites_empty = false;
                            }
                        }

                        if is_all_construction_sites_empty {
                            break;
                        }

                        if let Some(construction_sites) = team_construction_sites_iters.get_mut(&engineer.2.team) {
                            if let Some(construction_site) = construction_sites.next() {
                                if commands.get_entity(construction_site.1.current_builder).is_none() {
                                    construction_site.1.current_builder = engineer.0;
                                    commands.entity(engineer.0).try_insert(BusyEngineer(
                                        EngineerActions::Construction((construction_site.2.translation, construction_site.0, construction_site.1.build_distance)))
                                    );

                                    unstarted_tasks.0.push((
                                        TaskPoolTypes::Extra,
                                        (
                                            engineer.1.translation,
                                            construction_site.2.translation,
                                            Some(100.),
                                            engineer.0,
                                        ),
                                    ));
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
            }

            let mut team_deconstruction_sites: HashMap<i32, Vec<(Entity, Mut<'_, ToDeconstruct>, &Transform, Option<&BuildingConstructionSite>)>>
            = HashMap::new();
            let mut team_deconstruction_sites_iters: HashMap<i32, std::slice::IterMut<'_, (Entity, Mut<'_, ToDeconstruct>, &Transform, Option<&BuildingConstructionSite>)>>
            = HashMap::new();
            for deconstruction_site in deconstruction_sites_q.iter_mut() {
                team_deconstruction_sites.entry(deconstruction_site.1.team).or_insert_with(Vec::new).push(deconstruction_site);
            }

            let mut is_all_deconstruction_sites_empty = true;
            for deconstruction_sites in team_deconstruction_sites.iter_mut() {
                if !deconstruction_sites.1.is_empty(){
                    is_all_deconstruction_sites_empty = false;
                    team_deconstruction_sites_iters.entry(*deconstruction_sites.0).or_insert_with(|| deconstruction_sites.1.iter_mut());
                }
            }

            if !is_all_deconstruction_sites_empty {
                loop {
                    if let Some(engineer) = free_engineers_iter.next(){
                        is_all_deconstruction_sites_empty = true;

                        for deconstruction_sites in team_deconstruction_sites_iters.iter() {
                            if deconstruction_sites.1.len() > 0 {
                                is_all_deconstruction_sites_empty = false;
                            }
                        }

                        if is_all_deconstruction_sites_empty {
                            break;
                        }

                        if let Some(deconstruction_sites) = team_deconstruction_sites_iters.get_mut(&engineer.2.team) {
                            if let Some(deconstruction_site) = deconstruction_sites.next() {
                                if commands.get_entity(deconstruction_site.1.deconstructor_entity).is_some() {continue;}

                                if let Some(construction_site) = deconstruction_site.3 {
                                    if let Ok(current_enginer) = busy_engineers.get(construction_site.current_builder) {
                                        deconstruction_site.1.deconstructor_entity = current_enginer.0;
                                        commands.entity(current_enginer.0).try_insert(BusyEngineer(
                                            EngineerActions::Deconstruction((deconstruction_site.2.translation, deconstruction_site.0, deconstruction_site.1.deconstruction_distance)))
                                        );

                                        if deconstruction_site.2.translation.distance(current_enginer.1.translation) > deconstruction_site.1.deconstruction_distance {
                                            unstarted_tasks.0.push((
                                                TaskPoolTypes::Extra,
                                                (
                                                    current_enginer.1.translation,
                                                    deconstruction_site.2.translation,
                                                    Some(100.),
                                                    current_enginer.0,
                                                ),
                                            ));
                                        }

                                        continue;
                                    } else if let Ok(current_enginer) = free_engineers.get(construction_site.current_builder) {
                                        deconstruction_site.1.deconstructor_entity = current_enginer.0;
                                        commands.entity(current_enginer.0).try_insert(BusyEngineer(
                                            EngineerActions::Deconstruction((deconstruction_site.2.translation, deconstruction_site.0, deconstruction_site.1.deconstruction_distance)))
                                        );

                                        if deconstruction_site.2.translation.distance(current_enginer.1.translation) > deconstruction_site.1.deconstruction_distance {
                                            unstarted_tasks.0.push((
                                                TaskPoolTypes::Extra,
                                                (
                                                    current_enginer.1.translation,
                                                    deconstruction_site.2.translation,
                                                    Some(100.),
                                                    current_enginer.0,
                                                ),
                                            ));
                                        }

                                        continue;
                                    }
                                }

                                deconstruction_site.1.deconstructor_entity = engineer.0;
                                commands.entity(engineer.0).try_insert(BusyEngineer(
                                    EngineerActions::Deconstruction((deconstruction_site.2.translation, deconstruction_site.0, deconstruction_site.1.deconstruction_distance)))
                                );
                
                                unstarted_tasks.0.push((
                                    TaskPoolTypes::Extra,
                                    (
                                        engineer.1.translation,
                                        deconstruction_site.2.translation,
                                        Some(100.),
                                        engineer.0,
                                    ),
                                ));
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    }  
}

pub fn process_busy_engineers (
    mut engineers_q: Query<(&EngineerComponent, &BusyEngineer, &Transform, Entity, &mut UnitComponent, &CombatComponent), With<BusyEngineer>>,
    mut buildings_to_interact_q: (
        Query<(Entity, &BuildingBlueprint, &Transform),
        (With<BuildingBlueprint>, Without<BuildingConstructionSite>, Without<DeconstructableBuilding>, Without<ToDeconstruct>)>,
        Query<(Entity, &mut BuildingConstructionSite, &Transform),
        (With<BuildingConstructionSite>, Without<BuildingBlueprint>, Without<DeconstructableBuilding>, Without<ToDeconstruct>)>,
        Query<(Entity, &mut BuildingConstructionSite, &Transform, &ToDeconstruct),
        (With<BuildingConstructionSite>, With<ToDeconstruct>, Without<BuildingBlueprint>, Without<DeconstructableBuilding>)>,
        Query<(Entity, &mut DeconstructableBuilding, &Transform, &mut ToDeconstruct),
        (With<DeconstructableBuilding>, With<ToDeconstruct>, Without<BuildingConstructionSite>, Without<BuildingBlueprint>)>,
    ),
    mut commands: Commands,
    mut unactivated_blueprints: ResMut<UnactivatedBlueprints>,
    mut resource_zones_q: Query<(&mut ResourceZone, &Transform, Entity), With<ResourceZone>>,
    mut producers_q: (
        Query<(&mut MaterialsProductionComponent, &CombatComponent), Without<BusyEngineer>>,
        Query<(&GlobalTransform, &UnitProductionBuildingComponent), With<VehiclesProducer>>,
    ),
    mut tile_map: ResMut<UnitsTileMap>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    timer: ResMut<camera::TimerResource>,
    game_stage: Res<GameStage>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut materials: (
        Res<Assets<StandardMaterial>>,
        ResMut<InstancedMaterials>,
        ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    ),
    ui_button_nodes: Res<UiButtonNodes>,
){
    match game_stage.0 {
        GameStages::GameStarted => {
            if !engineers_q.is_empty() {
                for mut engineer in engineers_q.iter_mut() {
                    match engineer.1.0 {
                        EngineerActions::ActivateBlueprint(action) => {
                            if !engineer.4.path.is_empty() && engineer.2.translation.distance(action.0) <= action.2 {
                                engineer.4.path = Vec::new();
                            }
                        },
                        EngineerActions::Construction(action) => {
                            if !engineer.4.path.is_empty() && engineer.2.translation.distance(action.0) <= action.2 {
                                engineer.4.path = Vec::new();
                            }
                        },
                        EngineerActions::Deconstruction(action) => {
                            if !engineer.4.path.is_empty() && engineer.2.translation.distance(action.0) <= action.2 {
                                engineer.4.path = Vec::new();
                            }
                        },
                    }
                }
            
                if timer.0.finished() {
                    for engineer in engineers_q.iter() {
                        match engineer.1.0 {
                            EngineerActions::ActivateBlueprint(action) => {
                                if let Ok (current_blueprint) = buildings_to_interact_q.0.get(action.1) {
                                    if engineer.2.translation.distance(action.0) <= action.2 {
                                        let mut total_materials = 0;

                                        for material_producer in producers_q.0.iter() {
                                            if engineer.5.team != material_producer.1.team {continue;}

                                            total_materials += material_producer.0.available_materials;
                                        }

                                        if current_blueprint.1.resource_cost <= total_materials {
                                            let mut remaining_resource_cost = current_blueprint.1.resource_cost;

                                            for mut material_producer in producers_q.0.iter_mut() {
                                                if engineer.5.team != material_producer.1.team {continue;}

                                                let current_remains = remaining_resource_cost - material_producer.0.available_materials;

                                                if current_remains <= 0 {
                                                    material_producer.0.available_materials -= remaining_resource_cost;

                                                    break;
                                                } else {
                                                    remaining_resource_cost -= material_producer.0.available_materials;
                                                    material_producer.0.available_materials = 0;
                                                }
                                            }
                                        } else {
                                            continue;
                                        }

                                        let mut new_construction = Entity::PLACEHOLDER;
                                        let new_construction_tile = ((current_blueprint.2.translation.x / TILE_SIZE) as i32,
                                        (current_blueprint.2.translation.z / TILE_SIZE) as i32);

                                        let mut unit_type = UnitTypes::None;

                                        let color;
                                        if current_blueprint.1.team == 1 {
                                            color = Vec4::new(0., 0., 1., 1.);
                                        } else {
                                            color = Vec4::new(1., 0., 0., 1.);
                                        }

                                        let bar_size = ui_button_nodes.button_size * 0.75;
                
                                        match &current_blueprint.1.building_bundle {
                                            BuildingsBundles::InfantryBarracks(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }
    
                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::VehicleFactory(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }

                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::LogisticHub(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }

                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::ResourceMiner(bundle) => {
                                                let mut nearest_zone = (Entity::PLACEHOLDER, f32::INFINITY);

                                                for zone in resource_zones_q.iter() {
                                                    let distance = zone.1.translation.xz().distance(current_blueprint.2.translation.xz());
                                                    if distance < nearest_zone.1 {
                                                        nearest_zone.0 = zone.2;
                                                        nearest_zone.1 = distance;
                                                    }
                                                }

                                                if let Ok(mut zone) = resource_zones_q.get_mut(nearest_zone.0) {
                                                    let mut is_forbidden = false;

                                                    for miner in zone.0.current_miners.iter() {
                                                        if let Some(entity) = miner.1 {
                                                            if entity.0 != current_blueprint.0 && entity.1 != 0 && commands.get_entity(entity.0).is_some() {
                                                                is_forbidden = true;
                                                            }
                                                        }
                                                    }

                                                    if !is_forbidden {
                                                        let material;

                                                        if let Some(mat) =
                                                        materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                            materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                        }

                                                        new_construction = commands.spawn(MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_blueprint.2,
                                                            ..default()
                                                        }).try_insert(BuildingConstructionSite{
                                                            team: current_blueprint.1.team,
                                                            building_bundle: current_blueprint.1.building_bundle.clone(),
                                                            build_power_total: current_blueprint.1.build_power_remaining,
                                                            build_power_remaining: current_blueprint.1.build_power_remaining,
                                                            name: current_blueprint.1.name.clone(),
                                                            build_distance: current_blueprint.1.build_distance,
                                                            current_builder: engineer.3,
                                                            resource_cost: current_blueprint.1.resource_cost,
                                                        }).try_insert(CombatComponent{
                                                            team: current_blueprint.1.team,
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

                                                        for mut miner in zone.0.current_miners.iter_mut() {
                                                            miner.1 = &mut None;
                                                        }

                                                        if let Some(miner) = zone.0.current_miners.get_mut(&current_blueprint.1.team) {
                                                            *miner = Some((new_construction, 1));
                                                        }

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
                                                        .try_insert(Visibility::Hidden)
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
                                                            .try_insert(ConstructionProgressBar {
                                                                constrcution_entity: new_construction,
                                                                max_width: bar_size,
                                                            });
                                                        });
                                                    }
                                                }
                                            },
                                            BuildingsBundles::Pillbox(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }

                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::WatchingTower(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }

                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
                                                    current_health: bundle.combat_component.current_health / 10,
                                                    max_health: bundle.combat_component.current_health / 10,
                                                    unit_type: bundle.combat_component.unit_type.clone(),
                                                    attack_type: bundle.combat_component.attack_type.clone(),
                                                    attack_animation_type: bundle.combat_component.attack_animation_type.clone(),
                                                    attack_frequency: bundle.combat_component.attack_frequency,
                                                    attack_elapsed_time: bundle.combat_component.attack_elapsed_time,
                                                    detection_range: bundle.combat_component.detection_range / 10.,
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::Autoturret(bundle) => {
                                                let material;

                                                if let Some(mat) =
                                                materials.1.team_materials.get(&(bundle.model.mesh.id(), current_blueprint.1.team)) {
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

                                                    materials.1.team_materials.insert((bundle.model.mesh.id(), current_blueprint.1.team), material.clone());
                                                }

                                                let simplified_material;
                                                if current_blueprint.1.team == 1 {
                                                    simplified_material = materials.1.blue_solid.clone();
                                                } else {
                                                    simplified_material = materials.1.red_solid.clone();
                                                }

                                                new_construction = commands.spawn(MaterialMeshBundle{
                                                    mesh: bundle.model.mesh.clone(),
                                                    material: material.clone(),
                                                    transform: *current_blueprint.2,
                                                    ..default()
                                                }).try_insert(BuildingConstructionSite{
                                                    team: current_blueprint.1.team,
                                                    building_bundle: current_blueprint.1.building_bundle.clone(),
                                                    build_power_total: current_blueprint.1.build_power_remaining,
                                                    build_power_remaining: current_blueprint.1.build_power_remaining,
                                                    name: current_blueprint.1.name.clone(),
                                                    build_distance: current_blueprint.1.build_distance,
                                                    current_builder: engineer.3,
                                                    resource_cost: current_blueprint.1.resource_cost,
                                                }).try_insert(CombatComponent{
                                                    team: current_blueprint.1.team,
                                                    current_health: bundle.combat_component.current_health / 10,
                                                    max_health: bundle.combat_component.current_health / 10,
                                                    unit_type: bundle.combat_component.unit_type.clone(),
                                                    attack_type: AttackTypes::None,
                                                    attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                                                    attack_frequency: 0,
                                                    attack_elapsed_time: 0,
                                                    detection_range: bundle.combat_component.detection_range / 5.,
                                                    attack_range: 0.,
                                                    enemies: bundle.combat_component.enemies.clone(),
                                                    is_static: bundle.combat_component.is_static,
                                                    unit_data: (
                                                        new_construction_tile,
                                                        bundle.combat_component.unit_data.1.clone()
                                                    ),
                                                })
                                                .try_insert(LOD{
                                                    detailed: (bundle.model.mesh.clone(), Some(material.clone()), None),
                                                    simplified: (bundle.lod.mesh.clone(), simplified_material),
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
                                                .try_insert(Visibility::Hidden)
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
                                                    .try_insert(ConstructionProgressBar {
                                                        constrcution_entity: new_construction,
                                                        max_width: bar_size,
                                                    });
                                                });
                                            },
                                            BuildingsBundles::None => {},
                                        }

                                        if let Some(blueprints) = unactivated_blueprints.0.get_mut(&current_blueprint.1.team) {
                                            blueprints.remove(&current_blueprint.0);
                                        }

                                        if new_construction != Entity::PLACEHOLDER {
                                            commands.entity(current_blueprint.0).despawn();
                                            commands.entity(engineer.3).try_insert(BusyEngineer(EngineerActions::Construction((action.0, new_construction, action.2))));

                                            tile_map.tiles.entry(current_blueprint.1.team).or_insert_with(HashMap::new).entry(new_construction_tile)
                                            .or_insert_with(HashMap::new).insert(new_construction, (current_blueprint.2.translation, unit_type));

                                            if matches!(network_status.0, NetworkStatuses::Host) {
                                                let mut channel_id = 60;
                                                while channel_id <= 89 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ConstructionSiteBuilt {
                                                        team: current_blueprint.1.team,
                                                        name: current_blueprint.1.name.clone(),
                                                        position: current_blueprint.2.translation,
                                                        blueprint_server_entity: current_blueprint.0,
                                                        server_entity: new_construction,
                                                        angle: current_blueprint.2.rotation.to_euler(EulerRot::XYZ).1,
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
                                else {
                                    commands.entity(engineer.3).remove::<BusyEngineer>();

                                    let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                    for producer in producers_q.1.iter() {
                                        let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                        let distance = producer_pos.distance(engineer.2.translation);
                                        
                                        if distance < nearest_vehicles_producer.1 {
                                            nearest_vehicles_producer = (producer_pos, distance);
                                        }
                                    }

                                    if nearest_vehicles_producer.1 != f32::INFINITY {
                                        unstarted_tasks.0.push((
                                            TaskPoolTypes::Extra,
                                            (
                                                engineer.2.translation,
                                                nearest_vehicles_producer.0,
                                                Some(100.),
                                                engineer.3,
                                            ),
                                        ));
                                    }
                                }
                            },
                            EngineerActions::Construction(action) => {
                                if let Ok (mut current_construction_site) = buildings_to_interact_q.1.get_mut(action.1) {
                                    if engineer.2.translation.distance(action.0) <= action.2 {
                                        current_construction_site.1.build_power_remaining -= engineer.0.build_power;

                                        if matches!(network_status.0, NetworkStatuses::Host){
                                            let mut channel_id = 30;
                                            while channel_id <= 59 {
                                                if let Err(_) = server
                                                .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ConstructionProgressChanged {
                                                    server_entity: action.1,
                                                    current_build_power: current_construction_site.1.build_power_remaining,
                                                }){
                                                    channel_id += 1;
                                                } else {
                                                    break;
                                                }
                                            }
                                        }
                
                                        if current_construction_site.1.build_power_remaining <= 0 {
                                            let current_construction_site_tile = (
                                                (current_construction_site.2.translation.x / TILE_SIZE) as i32,
                                                (current_construction_site.2.translation.z / TILE_SIZE) as i32
                                            );

                                            let mut new_building = Entity::PLACEHOLDER;
                                            let mut unit_type = UnitTypes::None;

                                            let color;
                                            if current_construction_site.1.team == 1 {
                                                color = Vec4::new(0., 0., 1., 1.);
                                            } else {
                                                color = Vec4::new(1., 0., 0., 1.);
                                            }

                                            let bar_size = ui_button_nodes.button_size * 0.75;
                
                                            match &current_construction_site.1.building_bundle {
                                                BuildingsBundles::InfantryBarracks(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }
    
                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
                                                            ..default()
                                                        },
                                                        bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                        bundle.building_component.clone(),
                                                        CombatComponent{
                                                            team: current_construction_site.1.team,
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
                                                    ))
                                                    .try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .try_insert(SwitchableBuilding(true))
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
                                                    .try_insert(Visibility::Hidden)
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
                                                        .try_insert(HumanResourcesDisplay {
                                                            original_width: bar_size,
                                                            storage_entity: new_building,
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
                                                    .try_insert(Visibility::Hidden)
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
                                                        .try_insert(MaterialsDisplay {
                                                            original_width: bar_size,
                                                            storage_entity: new_building,
                                                        });
                                                    });
                                                },
                                                BuildingsBundles::VehicleFactory(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }

                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
                                                            ..default()
                                                        },
                                                        bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                        bundle.building_component.clone(),
                                                        CombatComponent{
                                                            team: current_construction_site.1.team,
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
                                                    )).try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .try_insert(SwitchableBuilding(true))
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
                                                    .try_insert(Visibility::Hidden)
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
                                                        .try_insert(HumanResourcesDisplay {
                                                            original_width: bar_size,
                                                            storage_entity: new_building,
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
                                                    .try_insert(Visibility::Hidden)
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
                                                        .try_insert(MaterialsDisplay {
                                                            original_width: bar_size,
                                                            storage_entity: new_building,
                                                        });
                                                    });
                                                },
                                                BuildingsBundles::LogisticHub(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }

                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
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
                                                            team: current_construction_site.1.team,
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
                                                    .try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .try_insert(SwitchableBuilding(true))
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
                                                    .try_insert(Visibility::Hidden)
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
                                                        .try_insert(MaterialsDisplay {
                                                            original_width: bar_size,
                                                            storage_entity: new_building,
                                                        });
                                                    });
                                                },
                                                BuildingsBundles::ResourceMiner(bundle) => {
                                                    let mut nearest_zone = (Entity::PLACEHOLDER, f32::INFINITY);

                                                    for zone in resource_zones_q.iter() {
                                                        let distance = zone.1.translation.xz().distance(current_construction_site.2.translation.xz());
                                                        if distance < nearest_zone.1 {
                                                            nearest_zone.0 = zone.2;
                                                            nearest_zone.1 = distance;
                                                        }
                                                    }
    
                                                    if let Ok(mut zone) = resource_zones_q.get_mut(nearest_zone.0) {
                                                        let mut is_forbidden = false;

                                                        for miner in zone.0.current_miners.iter() {
                                                            if let Some(entity) = miner.1 {
                                                                if entity.0 != current_construction_site.0 && entity.1 != 0 && commands.get_entity(entity.0).is_some() {
                                                                    is_forbidden = true;
                                                                }
                                                            }
                                                        }

                                                        if !is_forbidden {
                                                            let material;

                                                            if let Some(mat) =
                                                            materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                                materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                            }

                                                            new_building = commands.spawn((
                                                                MaterialMeshBundle{
                                                                    mesh: bundle.model.mesh.clone(),
                                                                    material: material.clone(),
                                                                    transform: *current_construction_site.2,
                                                                    ..default()
                                                                },
                                                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                                bundle.building_component.clone(),
                                                                CombatComponent{
                                                                    team: current_construction_site.1.team,
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
                                                            .try_insert(NavMeshAffector)
                                                            .try_insert(NavMeshAreaType(None))
                                                            .try_insert(DeconstructableBuilding{
                                                                team: current_construction_site.1.team,
                                                                materials_spent: current_construction_site.1.resource_cost,
                                                                buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                                buildpower_to_deconstruct_remaining: 0,
                                                                deconstruction_distance: action.2,
                                                            })
                                                            .id();

                                                            for mut miner in zone.0.current_miners.iter_mut() {
                                                                miner.1 = &mut None;
                                                            }

                                                            if let Some(miner) = zone.0.current_miners.get_mut(&current_construction_site.1.team) {
                                                                *miner = Some((new_building, 1));
                                                            }

                                                            unit_type = bundle.combat_component.unit_type;
                                                        }
                                                    }
                                                },
                                                BuildingsBundles::Pillbox(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }

                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
                                                            ..default()
                                                        },
                                                        CoverComponent{
                                                            cover_efficiency: bundle.building_component.cover_efficiency,
                                                            points: bundle.building_component.points.clone(),
                                                            units_inside: bundle.building_component.units_inside.clone(),
                                                        },
                                                        bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                        CombatComponent{
                                                            team: current_construction_site.1.team,
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
                                                    .try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .id();

                                                    unit_type = bundle.combat_component.unit_type;
                                                },
                                                BuildingsBundles::WatchingTower(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }

                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
                                                            ..default()
                                                        },
                                                        bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                        CombatComponent{
                                                            team: current_construction_site.1.team,
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
                                                    .try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .id();

                                                    unit_type = bundle.combat_component.unit_type;
                                                },
                                                BuildingsBundles::Autoturret(bundle) => {
                                                    let material;

                                                    if let Some(mat) =
                                                    materials.1.team_materials.get(&(bundle.model.mesh.id(), current_construction_site.1.team)) {
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

                                                        materials.1.team_materials.insert((bundle.model.mesh.id(), current_construction_site.1.team), material.clone());
                                                    }

                                                    let simplified_material;
                                                    if current_construction_site.1.team == 1 {
                                                        simplified_material = materials.1.blue_solid.clone();
                                                    } else {
                                                        simplified_material = materials.1.red_solid.clone();
                                                    }

                                                    new_building = commands.spawn((
                                                        MaterialMeshBundle{
                                                            mesh: bundle.model.mesh.clone(),
                                                            material: material.clone(),
                                                            transform: *current_construction_site.2,
                                                            ..default()
                                                        },
                                                        bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                                        CombatComponent{
                                                            team: current_construction_site.1.team,
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
                                                    .try_insert(NavMeshAffector)
                                                    .try_insert(NavMeshAreaType(None))
                                                    .try_insert(DeconstructableBuilding{
                                                        team: current_construction_site.1.team,
                                                        materials_spent: current_construction_site.1.resource_cost,
                                                        buildpower_to_deconstruct_total: current_construction_site.1.build_power_total,
                                                        buildpower_to_deconstruct_remaining: 0,
                                                        deconstruction_distance: action.2,
                                                    })
                                                    .try_insert(LOD{
                                                        detailed: (bundle.model.mesh.clone(), Some(material.clone()), None),
                                                        simplified: (bundle.lod.mesh.clone(), simplified_material),
                                                    })
                                                    .id();

                                                    unit_type = bundle.combat_component.unit_type;
                                                },
                                                BuildingsBundles::None => {},
                                            }
                
                                            commands.entity(current_construction_site.0).despawn();
                                            commands.entity(engineer.3).remove::<BusyEngineer>();

                                            let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                            for producer in producers_q.1.iter() {
                                                let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                                let distance = producer_pos.distance(engineer.2.translation);
                                                
                                                if distance < nearest_vehicles_producer.1 {
                                                    nearest_vehicles_producer = (producer_pos, distance);
                                                }
                                            }

                                            if nearest_vehicles_producer.1 != f32::INFINITY {
                                                unstarted_tasks.0.push((
                                                    TaskPoolTypes::Extra,
                                                    (
                                                        engineer.2.translation,
                                                        nearest_vehicles_producer.0,
                                                        Some(100.),
                                                        engineer.3,
                                                    ),
                                                ));
                                            }

                                            tile_map.tiles.entry(current_construction_site.1.team).or_insert_with(HashMap::new).entry(current_construction_site_tile)
                                            .or_insert_with(HashMap::new).remove(&current_construction_site.0);

                                            if new_building != Entity::PLACEHOLDER {
                                                tile_map.tiles.entry(current_construction_site.1.team).or_insert_with(HashMap::new).entry(current_construction_site_tile)
                                                .or_insert_with(HashMap::new).insert(new_building, (current_construction_site.2.translation, unit_type));

                                                if matches!(network_status.0, NetworkStatuses::Host){
                                                    let mut channel_id = 60;
                                                    while channel_id <= 89 {
                                                        if let Err(_) = server
                                                        .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::BuildingBuilt {
                                                            team: current_construction_site.1.team,
                                                            name: current_construction_site.1.name.clone(),
                                                            position: current_construction_site.2.translation,
                                                            construction_site_server_entity: current_construction_site.0,
                                                            server_entity: new_building,
                                                            angle: current_construction_site.2.rotation.to_euler(EulerRot::XYZ).1,
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
                                else if let Err(_) = buildings_to_interact_q.2.get(action.1) {
                                    commands.entity(engineer.3).remove::<BusyEngineer>();

                                    let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                    for producer in producers_q.1.iter() {
                                        let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                        let distance = producer_pos.distance(engineer.2.translation);
                                        
                                        if distance < nearest_vehicles_producer.1 {
                                            nearest_vehicles_producer = (producer_pos, distance);
                                        }
                                    }

                                    if nearest_vehicles_producer.1 != f32::INFINITY {
                                        unstarted_tasks.0.push((
                                            TaskPoolTypes::Extra,
                                            (
                                                engineer.2.translation,
                                                nearest_vehicles_producer.0,
                                                Some(100.),
                                                engineer.3,
                                            ),
                                        ));
                                    }
                                }
                            },
                            EngineerActions::Deconstruction(action) => {
                                if let Ok(mut current_construction_site) = buildings_to_interact_q.2.get_mut(action.1) {
                                    if engineer.2.translation.distance(action.0) <= action.2 {
                                        if current_construction_site.1.build_power_remaining < current_construction_site.1.build_power_total {
                                            current_construction_site.1.build_power_remaining += engineer.0.build_power;

                                            if matches!(network_status.0, NetworkStatuses::Host){
                                                let mut channel_id = 30;
                                                while channel_id <= 59 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ConstructionProgressChanged {
                                                        server_entity: action.1,
                                                        current_build_power: current_construction_site.1.build_power_remaining,
                                                    }){
                                                        channel_id += 1;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }
                                        } else {
                                            commands.entity(action.1).despawn();

                                            let mut materials_to_return_remaining = current_construction_site.1.resource_cost;

                                            for mut material_producer in producers_q.0.iter_mut() {
                                                if engineer.5.team != material_producer.1.team {continue;}

                                                let free_storage = material_producer.0.materials_storage_capacity - material_producer.0.available_materials;

                                                if free_storage < materials_to_return_remaining {
                                                    material_producer.0.available_materials = material_producer.0.materials_storage_capacity;
                                                    materials_to_return_remaining -= free_storage;
                                                } else {
                                                    material_producer.0.available_materials += materials_to_return_remaining;

                                                    break;
                                                }
                                            }

                                            if matches!(network_status.0, NetworkStatuses::Host){
                                                let mut channel_id = 30;
                                                while channel_id <= 59 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                                        server_entity: action.1, 
                                                    }){
                                                        channel_id += 1;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }

                                            commands.entity(engineer.3).remove::<BusyEngineer>();

                                            let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                            for producer in producers_q.1.iter() {
                                                let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                                let distance = producer_pos.distance(engineer.2.translation);
                                                
                                                if distance < nearest_vehicles_producer.1 {
                                                    nearest_vehicles_producer = (producer_pos, distance);
                                                }
                                            }

                                            if nearest_vehicles_producer.1 != f32::INFINITY {
                                                unstarted_tasks.0.push((
                                                    TaskPoolTypes::Extra,
                                                    (
                                                        engineer.2.translation,
                                                        nearest_vehicles_producer.0,
                                                        Some(100.),
                                                        engineer.3,
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                } else if let Ok(mut current_building) = buildings_to_interact_q.3.get_mut(action.1) {
                                    if engineer.2.translation.distance(action.0) <= action.2 {
                                        if current_building.3.progress_bar_entity == Entity::PLACEHOLDER {
                                            let bar_size = ui_button_nodes.button_size * 0.75;

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
                                            .try_insert(Visibility::Hidden)
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
                                                .try_insert(ConstructionProgressBar {
                                                    constrcution_entity: current_building.0,
                                                    max_width: bar_size,
                                                });
                                            }).id();

                                            current_building.3.progress_bar_entity = bar;

                                            if matches!(network_status.0, NetworkStatuses::Host){
                                                let mut channel_id = 30;
                                                while channel_id <= 59 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::DeconstructionStarted {
                                                        server_deconstruction_entity: current_building.0,
                                                    }){
                                                        channel_id += 1;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        
                                        if current_building.1.buildpower_to_deconstruct_remaining < current_building.1.buildpower_to_deconstruct_total {
                                            current_building.1.buildpower_to_deconstruct_remaining += engineer.0.build_power;

                                            if matches!(network_status.0, NetworkStatuses::Host){
                                                let mut channel_id = 30;
                                                while channel_id <= 59 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::DeconstructionProgressChanged {
                                                        server_entity: action.1,
                                                        current_build_power: current_building.1.buildpower_to_deconstruct_remaining,
                                                    }){
                                                        channel_id += 1;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }
                                        } else {
                                            commands.entity(action.1).despawn();

                                            let mut materials_to_return_remaining = current_building.1.materials_spent;

                                            for mut material_producer in producers_q.0.iter_mut() {
                                                if engineer.5.team != material_producer.1.team {continue;}

                                                let free_storage = material_producer.0.materials_storage_capacity - material_producer.0.available_materials;

                                                if free_storage < materials_to_return_remaining {
                                                    material_producer.0.available_materials = material_producer.0.materials_storage_capacity;
                                                    materials_to_return_remaining -= free_storage;
                                                } else {
                                                    material_producer.0.available_materials += materials_to_return_remaining;

                                                    break;
                                                }
                                            }

                                            if matches!(network_status.0, NetworkStatuses::Host){
                                                let mut channel_id = 30;
                                                while channel_id <= 59 {
                                                    if let Err(_) = server
                                                    .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                                                        server_entity: action.1, 
                                                    }){
                                                        channel_id += 1;
                                                    } else {
                                                        break;
                                                    }
                                                }
                                            }

                                            commands.entity(engineer.3).remove::<BusyEngineer>();

                                            let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                            for producer in producers_q.1.iter() {
                                                let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                                let distance = producer_pos.distance(engineer.2.translation);
                                                
                                                if distance < nearest_vehicles_producer.1 {
                                                    nearest_vehicles_producer = (producer_pos, distance);
                                                }
                                            }

                                            if nearest_vehicles_producer.1 != f32::INFINITY {
                                                unstarted_tasks.0.push((
                                                    TaskPoolTypes::Extra,
                                                    (
                                                        engineer.2.translation,
                                                        nearest_vehicles_producer.0,
                                                        Some(100.),
                                                        engineer.3,
                                                    ),
                                                ));
                                            }
                                        }
                                    }
                                } else {
                                    commands.entity(engineer.3).remove::<BusyEngineer>();

                                    let mut nearest_vehicles_producer = (Vec3::ZERO, f32::INFINITY);

                                    for producer in producers_q.1.iter() {
                                        let producer_pos = producer.0.transform_point(producer.1.spawn_point);

                                        let distance = producer_pos.distance(engineer.2.translation);
                                        
                                        if distance < nearest_vehicles_producer.1 {
                                            nearest_vehicles_producer = (producer_pos, distance);
                                        }
                                    }

                                    if nearest_vehicles_producer.1 != f32::INFINITY {
                                        unstarted_tasks.0.push((
                                            TaskPoolTypes::Extra,
                                            (
                                                engineer.2.translation,
                                                nearest_vehicles_producer.0,
                                                Some(100.),
                                                engineer.3,
                                            ),
                                        ));
                                    }
                                }
                            },
                        }
                    }
                }
            }
        },
        _ => {
            for blueprint in buildings_to_interact_q.0.iter() {
                let current_construction_site_tile = (
                    (blueprint.2.translation.x / TILE_SIZE) as i32,
                    (blueprint.2.translation.z / TILE_SIZE) as i32
                );

                let mut new_building = Entity::PLACEHOLDER;
                let mut unit_type = UnitTypes::None;

                let color;
                if blueprint.1.team == 1 {
                    color = Vec4::new(0., 0., 1., 1.);
                } else {
                    color = Vec4::new(1., 0., 0., 1.);
                }

                let bar_size = ui_button_nodes.button_size * 0.75;

                match &blueprint.1.building_bundle {
                    BuildingsBundles::InfantryBarracks(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }
    
                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
                                ..default()
                            },
                            bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                            bundle.building_component.clone(),
                            CombatComponent{
                                team: blueprint.1.team,
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
                        ))
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .try_insert(SwitchableBuilding(true))
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
                        .try_insert(Visibility::Hidden)
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
                            .try_insert(HumanResourcesDisplay {
                                original_width: bar_size,
                                storage_entity: new_building,
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
                        .try_insert(Visibility::Hidden)
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
                            .try_insert(MaterialsDisplay {
                                original_width: bar_size,
                                storage_entity: new_building,
                            });
                        });
                    },
                    BuildingsBundles::VehicleFactory(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }

                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
                                ..default()
                            },
                            bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                            bundle.building_component.clone(),
                            CombatComponent{
                                team: blueprint.1.team,
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
                        ))
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .try_insert(SwitchableBuilding(true))
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
                        .try_insert(Visibility::Hidden)
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
                            .try_insert(HumanResourcesDisplay {
                                original_width: bar_size,
                                storage_entity: new_building,
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
                        .try_insert(Visibility::Hidden)
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
                            .try_insert(MaterialsDisplay {
                                original_width: bar_size,
                                storage_entity: new_building,
                            });
                        });
                    },
                    BuildingsBundles::LogisticHub(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }

                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
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
                                team: blueprint.1.team,
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
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .try_insert(SwitchableBuilding(true))
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
                        .try_insert(Visibility::Hidden)
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
                            .try_insert(MaterialsDisplay {
                                original_width: bar_size,
                                storage_entity: new_building,
                            });
                        });
                    },
                    BuildingsBundles::ResourceMiner(bundle) => {
                        let mut nearest_zone = (Entity::PLACEHOLDER, f32::INFINITY);

                        for zone in resource_zones_q.iter() {
                            let distance = zone.1.translation.xz().distance(blueprint.2.translation.xz());
                            if distance < nearest_zone.1 {
                                nearest_zone.0 = zone.2;
                                nearest_zone.1 = distance;
                            }
                        }

                        if let Ok(mut zone) = resource_zones_q.get_mut(nearest_zone.0) {
                            let mut is_forbidden = false;

                            for miner in zone.0.current_miners.iter() {
                                if let Some(entity) = miner.1 {
                                    if entity.0 != blueprint.0 && entity.1 != 0 && commands.get_entity(entity.0).is_some() {
                                        is_forbidden = true;
                                    }
                                }
                            }

                            let material;

                            if let Some(mat) =
                            materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                                materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                            }

                            new_building = commands.spawn((
                                MaterialMeshBundle{
                                    mesh: bundle.model.mesh.clone(),
                                    material: material.clone(),
                                    transform: *blueprint.2,
                                    ..default()
                                },
                                bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                                bundle.building_component.clone(),
                                CombatComponent{
                                    team: blueprint.1.team,
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
                            .try_insert(NavMeshAffector)
                            .try_insert(NavMeshAreaType(None))
                            .try_insert(DeconstructableBuilding{
                                team: blueprint.1.team,
                                materials_spent: blueprint.1.resource_cost,
                                buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                                buildpower_to_deconstruct_remaining: 0,
                                deconstruction_distance: blueprint.1.build_distance,
                            })
                            .id();

                            for mut miner in zone.0.current_miners.iter_mut() {
                                miner.1 = &mut None;
                            }

                            if let Some(miner) = zone.0.current_miners.get_mut(&blueprint.1.team) {
                                *miner = Some((new_building, 1));
                            }

                            unit_type = bundle.combat_component.unit_type;
                        }
                    },
                    BuildingsBundles::Pillbox(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }

                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
                                ..default()
                            },
                            bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                            CoverComponent{
                                cover_efficiency: bundle.building_component.cover_efficiency,
                                points: bundle.building_component.points.clone(),
                                units_inside: bundle.building_component.units_inside.clone(),
                            },
                            CombatComponent{
                                team: blueprint.1.team,
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
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .id();

                        unit_type = bundle.combat_component.unit_type;
                    },
                    BuildingsBundles::WatchingTower(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }

                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
                                ..default()
                            },
                            bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                            CombatComponent{
                                team: blueprint.1.team,
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
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .id();

                        unit_type = bundle.combat_component.unit_type;
                    },
                    BuildingsBundles::Autoturret(bundle) => {
                        let material;

                        if let Some(mat) =
                        materials.1.team_materials.get(&(bundle.model.mesh.id(), blueprint.1.team)) {
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

                            materials.1.team_materials.insert((bundle.model.mesh.id(), blueprint.1.team), material.clone());
                        }

                        let simplified_material;
                        if blueprint.1.team == 1 {
                            simplified_material = materials.1.blue_solid.clone();
                        } else {
                            simplified_material = materials.1.red_solid.clone();
                        }

                        new_building = commands.spawn((
                            MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: *blueprint.2,
                                ..default()
                            },
                            bundle.collider.clone(), CollisionGroups::new(Group::GROUP_2, Group::all()),
                            CombatComponent{
                                team: blueprint.1.team,
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
                        .try_insert(NavMeshAffector)
                        .try_insert(NavMeshAreaType(None))
                        .try_insert(DeconstructableBuilding{
                            team: blueprint.1.team,
                            materials_spent: blueprint.1.resource_cost,
                            buildpower_to_deconstruct_total: blueprint.1.build_power_remaining,
                            buildpower_to_deconstruct_remaining: 0,
                            deconstruction_distance: blueprint.1.build_distance,
                        })
                        .try_insert(LOD{
                            detailed: (bundle.model.mesh.clone(), Some(material.clone()), None),
                            simplified: (bundle.lod.mesh.clone(), simplified_material),
                        })
                        .id();

                        unit_type = bundle.combat_component.unit_type;
                    },
                    BuildingsBundles::None => {},
                }

                tile_map.tiles.entry(blueprint.1.team).or_insert_with(HashMap::new).entry(current_construction_site_tile)
                .or_insert_with(HashMap::new).remove(&blueprint.0);

                commands.entity(blueprint.0).despawn();

                if new_building != Entity::PLACEHOLDER {
                    tile_map.tiles.entry(blueprint.1.team).or_insert_with(HashMap::new).entry(current_construction_site_tile)
                    .or_insert_with(HashMap::new).insert(new_building, (blueprint.2.translation, unit_type));

                    if matches!(network_status.0, NetworkStatuses::Host){
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::BuildingBuilt {
                                team: blueprint.1.team,
                                name: blueprint.1.name.clone(),
                                position: blueprint.2.translation,
                                construction_site_server_entity: blueprint.0,
                                server_entity: new_building,
                                angle: blueprint.2.rotation.to_euler(EulerRot::XYZ).1,
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
}

pub fn generate_parabolic_trajectory(
    start: Vec3,
    end: Vec3,
    height: f32,
    num_points: usize,
) -> Vec<Vec3> {
    let mut points = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let t = i as f32 / (num_points - 1) as f32;
        let x = start.x + t * (end.x - start.x);
        let z = start.z + t * (end.z - start.z);
        let y = height * (1.0 - (2.0 * t - 1.0).powi(2));

        points.push(Vec3::new(x, y, z));
    }

    points
}

pub fn toggle_artillery_designation(
    mut artillery_designation: ResMut<IsArtilleryDesignationActive>,
    mut is_unit_deselection_allowed: ResMut<IsUnitDeselectionAllowed>,
    mut ui_blocker: ResMut<UiBlocker>,
    mut artillery_units_q: Query<(Entity, &mut ArtilleryUnit), (With<ArtilleryUnit>, With<SelectedUnit>)>,
    mut event_reader: (
        EventReader<ToggleArtilleryDesignation>,
        EventReader<CancelArtilleryTargets>,
        EventReader<MoveOrderEvent>,
    ),
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    for _event in event_reader.0.read() {
        if !artillery_designation.0 {
            artillery_designation.0 = true;
            is_unit_deselection_allowed.0 = false;
        }

        ui_blocker.is_bottom_left_node_blocked = true;
    }

    for _event in event_reader.1.read() {
        artillery_designation.0 = false;
        is_unit_deselection_allowed.0 = true;

        for mut artillery_unit in artillery_units_q.iter_mut() {
            commands.entity(artillery_unit.0).remove::<ArtilleryNeedsToFire>();
            artillery_unit.1.elapsed_reload_time = 0;

            if matches!(network_status.0, NetworkStatuses::Client) {
                if let Some(server_entity) = entity_maps.client_to_server.get(&artillery_unit.0) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::CancelArtilleryFire {
                            artillery_entity: *server_entity,
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

    for _event in event_reader.2.read() {
        artillery_designation.0 = false;
        is_unit_deselection_allowed.0 = true;

        for mut artillery_unit in artillery_units_q.iter_mut() {
            commands.entity(artillery_unit.0).remove::<ArtilleryNeedsToFire>();
            artillery_unit.1.elapsed_reload_time = 0;

            if matches!(network_status.0, NetworkStatuses::Client) {
                if let Some(server_entity) = entity_maps.client_to_server.get(&artillery_unit.0) {
                    let mut channel_id = 30;
                    while channel_id <= 59 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::CancelArtilleryFire {
                            artillery_entity: *server_entity,
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

pub fn artillery_designation_system (
    mut artillery_designation: ResMut<IsArtilleryDesignationActive>,
    is_unit_deselection_allowed: Res<IsUnitDeselectionAllowed>,
    mut artillery_units_q: Query<(Entity, &mut Transform, &mut UnitComponent), (With<ArtilleryUnit>, With<SelectedUnit>)>,
    ui_blocker: Res<UiBlocker>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    mut commands: Commands,
    mut event_writer: (
        //EventWriter<UnsentClientMessage>,
        EventWriter<ArtilleryOrderGiven>,
    ),
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    if artillery_designation.0 && !artillery_units_q.is_empty() {
        if mouse_buttons.just_pressed(MouseButton::Left) {
            if ui_blocker.is_bottom_left_node_blocked {return;}
            if let Some(cursor_ray) = **cursor_ray {
                let hits = raycast.cast_ray(cursor_ray, &default());
        
                if hits.len() > 0 {
                    if matches!(network_status.0, NetworkStatuses::Client) {
                        for mut artillery_unit in artillery_units_q.iter_mut() {
                            if let Some(server_entity) = entity_maps.client_to_server.get(&artillery_unit.0) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::ArtilleryDesignationRequest {
                                        artillery_entity: *server_entity,
                                        target_position: hits[0].1.position(),
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }

                            artillery_unit.2.path = vec![];

                            artillery_unit.1.look_at(
                                Vec3::new(
                                    hits[0].1.position().x,
                                    0.,
                                    hits[0].1.position().z,
                                ),
                                Vec3::Y,
                            );
                        }
                    } else {
                        for mut artillery_unit in artillery_units_q.iter_mut() {
                            commands.entity(artillery_unit.0).try_insert(ArtilleryNeedsToFire(hits[0].1.position()));

                            artillery_unit.2.path = vec![];

                            artillery_unit.1.look_at(
                                Vec3::new(
                                    hits[0].1.position().x,
                                    0.,
                                    hits[0].1.position().z,
                                ),
                                Vec3::Y,
                            );
                        }
                    }
                    
                    artillery_designation.0 = false;
                }
            }
        }
    } else if artillery_designation.0 && artillery_units_q.is_empty() {
        artillery_designation.0 = false;
    } else if !artillery_units_q.is_empty() && !is_unit_deselection_allowed.0 && mouse_buttons.just_released(MouseButton::Left){
        event_writer.0.send(ArtilleryOrderGiven);
    }
}

pub fn artillery_order_delayed (
    mut is_unit_deselection_allowed: ResMut<IsUnitDeselectionAllowed>,
    mut event_reader: EventReader<ArtilleryOrderGiven>,
){
    for _event in event_reader.read() {
        is_unit_deselection_allowed.0 = true;
    }
}

pub fn artillery_firing_system(
    mut artillery_units_q: Query<(&Transform, &mut ArtilleryUnit, &ArtilleryNeedsToFire, Entity, Option<&NeedToMove>, &SuppliesConsumerComponent), With<ArtilleryNeedsToFire>>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    instanced_materials: Res<InstancedMaterials>,
    attack_visualisation_assets: Res<AttackVisualisationAssets>,
    mut commands: Commands,
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    // mut event_writer: (
    //     EventWriter<UnsentServerMessage>,
    // ),
){
    let mut counter = 0;
    for mut artillery_unit in artillery_units_q.iter_mut() {
        if let None = artillery_unit.4 {
            if artillery_unit.0.translation.distance(artillery_unit.2.0) > artillery_unit.1.max_range {
                unstarted_tasks.0.push((
                    TaskPoolTypes::Extra,
                    (
                        artillery_unit.0.translation,
                        (artillery_unit.0.translation - artillery_unit.2.0).normalize() * (artillery_unit.1.max_range - 5.) + artillery_unit.2.0,
                        Some(100.),
                        artillery_unit.3,
                    ),
                ));
            }
            else {
                artillery_unit.1.elapsed_reload_time += time.delta().as_millis();
    
                if artillery_unit.1.elapsed_reload_time >= artillery_unit.1.reload_time {
                    artillery_unit.1.elapsed_reload_time = 0;

                    if artillery_unit.5.supplies <= 0 {
                        continue;
                    }

                    let mut rng = rand::thread_rng();
                    let mut end_point = artillery_unit.2.0;
                    if artillery_unit.1.accuracy != 0. {
                        end_point.x += rng.gen_range(-artillery_unit.1.accuracy..artillery_unit.1.accuracy);
                        end_point.z += rng.gen_range(-artillery_unit.1.accuracy..artillery_unit.1.accuracy);
                    }

                    let path = generate_parabolic_trajectory(
                        artillery_unit.0.translation,
                        end_point,
                        artillery_unit.1.peak_trajectory_height,
                        artillery_unit.1.trajectory_points,
                    );
    
                    let shell_entity = commands.spawn(MaterialMeshBundle {
                        mesh: attack_visualisation_assets.shell.0.clone(),
                        material: attack_visualisation_assets.shell.1.clone(),
                        transform: Transform::from_translation(artillery_unit.0.translation).looking_at(path[1], Vec3::Y),
                        ..default()
                    })
                    .try_insert(TrailEmmiterComponent)
                    .try_insert(BallisticProjectile{
                        path: path,
                        speed: artillery_unit.1.shell_speed,
                        start_position: artillery_unit.0.translation,
                        target_position: end_point,
                        waypoint_radius: artillery_unit.1.projectile_waypoints_check_factor,
                        elapsed: 0.,
                        inv_duration: 0.,
                        direct_damage: artillery_unit.1.direct_damage,
                        splash_damage: artillery_unit.1.splash_damage,
                        
                    }).id();

                    let mesh_handle = meshes.add(Triangle3d{
                        vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
                    });

                    commands.spawn(MaterialMeshBundle{
                        mesh: mesh_handle.clone(),
                        material: instanced_materials.red_solid.clone(),
                        transform: Transform::from_translation(artillery_unit.0.translation),
                        ..default()
                    })
                    .try_insert(
                        TrailComponent{
                            positions: vec![],
                            length: 10,
                            width: 0.05,
                            mesh_handle,
                            emmiter_entity: shell_entity,
                        }
                    );

                    counter += 1;

                    if counter <= 3 {
                        commands.entity(artillery_unit.3).try_insert(
                            AudioBundle{
                                source: attack_visualisation_assets.tank_shot_sound.clone(),
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

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ArtilleryProjectileSpawned {
                                position: artillery_unit.0.translation,
                                server_entity: shell_entity,
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
}

pub fn artillery_shells_movement_system(
    mut artillery_shells_q: Query<(Entity, &mut Transform, &mut BallisticProjectile), With<BallisticProjectile>>,
    mut commands: Commands,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<ExplosionEvent>,
    ),
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for mut artillery_shell in artillery_shells_q.iter_mut(){
        if !artillery_shell.2.path.is_empty(){
            let unit_position = artillery_shell.1.translation;
            let target_position = artillery_shell.2.path[0];

            if artillery_shell.2.elapsed == 0. {
                artillery_shell.1.look_at(
                    target_position,
                    Vec3::Y,
                );
                
                artillery_shell.2.start_position = unit_position;
                let distance = artillery_shell.2.start_position.xz().distance(target_position.xz());

                if distance <= artillery_shell.2.waypoint_radius {
                    artillery_shell.2.path.remove(0);
                } else {
                    let duration = distance / artillery_shell.2.speed;
                    artillery_shell.2.inv_duration = 1. / duration;

                    artillery_shell.2.elapsed += time.delta_seconds();
                    let t = artillery_shell.2.elapsed * artillery_shell.2.inv_duration;

                    let new_pos = if t >= 1. {
                        target_position
                    } else {
                        artillery_shell.2.start_position.lerp(target_position, t)
                    };

                    artillery_shell.1.translation = new_pos;

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 0;
                        while channel_id <= 29 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                                server_entity: artillery_shell.0,
                                new_position: new_pos,
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            } else {
                if unit_position.xz().distance(target_position.xz()) <= artillery_shell.2.waypoint_radius {
                    artillery_shell.2.path.remove(0);
                    artillery_shell.2.elapsed = 0.;
                } else {
                    artillery_shell.2.elapsed += time.delta_seconds();
                    let t = artillery_shell.2.elapsed * artillery_shell.2.inv_duration;

                    let new_pos = if t >= 1.0 {
                        target_position
                    } else {
                        artillery_shell.2.start_position.lerp(target_position, t)
                    };

                    artillery_shell.1.translation = new_pos;

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 0;
                        while channel_id <= 29 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                                server_entity: artillery_shell.0,
                                new_position: new_pos,
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            commands.entity(artillery_shell.0).despawn();

            event_writer.0.send(ExplosionEvent((artillery_shell.2.target_position, artillery_shell.2.direct_damage, artillery_shell.2.splash_damage)));

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 60;
                while channel_id <= 89 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                        server_entity: artillery_shell.0,
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }

                channel_id = 30;

                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ExplosionOccured {
                        position: artillery_shell.2.target_position,
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

pub fn game_starting_system (
    mut event_reader: EventReader<GameStartedEvent>,
    mut game_stage: ResMut<GameStage>,
    infantry_producers_q: Query<(Entity, &mut UnitProductionBuildingComponent, &Transform, &CombatComponent), (With<InfantryProducer>, Without<VehiclesProducer>)>,
    vehicles_producers_q: Query<(Entity, &mut UnitProductionBuildingComponent, &Transform, &CombatComponent), (With<VehiclesProducer>, Without<InfantryProducer>)>,
    delete_after_start_q: Query<Entity, With<DeleteAfterStart>>,
    mut production_queue: ResMut<ProductionQueue>,
    mut production_states: ResMut<ProductionState>,
    mut armies: ResMut<Armies>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut instanced_materials: ResMut<InstancedMaterials>,
    materials: Res<Assets<StandardMaterial>>,
    mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
){
    for _event in event_reader.read(){
        for to_delete in delete_after_start_q.iter() {
            commands.entity(to_delete).despawn();
        }

        let infantry_producers_q_collect: Vec<_> = infantry_producers_q.iter().collect();
        let vehicles_producers_q_collect: Vec<_> = vehicles_producers_q.iter().collect();

        let mut infantry_producers_q_cycle = infantry_producers_q_collect.iter().cycle();
        let mut vehicles_producers_q_cycle = vehicles_producers_q_collect.iter().cycle();

        for team_queue in production_queue.0.iter_mut(){
            let color;
            let simplified_material;
            if team_queue.0.clone() == 1 {
                color = Vec4::new(0., 0., 1., 1.);
                simplified_material = instanced_materials.blue_solid.clone();
            } else {
                color = Vec4::new(1., 0., 0., 1.);
                simplified_material = instanced_materials.red_solid.clone();
            }

            for unit_to_produce in team_queue.1.regular_infantry_queue.iter() {
                let mut is_produced = false;
                while is_produced == false {
                    if let Some(infantry_producer) = infantry_producers_q_cycle.next(){
                        if *team_queue.0 == infantry_producer.3.team {
                            is_produced = true;
                        } else {
                            continue;
                        }

                        if let Some(bundle) = infantry_producer.1.available_to_build.get(&unit_to_produce.1.0) {
                            let mut new_unit = Entity::PLACEHOLDER;

                            let battalion_type;
                            let unit_type;

                            let point = infantry_producer.2.transform_point(infantry_producer.1.spawn_point);
                            let tile = (i32::MAX, i32::MAX);
        
                            match &bundle.0 {
                                UnitBundles::Soldier(b) => {
                                    battalion_type = CompanyTypes::Regular;
                                    unit_type = b.combat_component.unit_type.clone();

                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Shock;
                                    unit_type = b.combat_component.unit_type.clone();

                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                _ => {
                                    battalion_type = CompanyTypes::None;
                                    unit_type = UnitTypes::None;
                                },
                            }
        
                            if let Some(platoon) = armies.0.get_mut(team_queue.0).unwrap().regular_squads.get_mut(&(
                                unit_to_produce.0.0,
                                unit_to_produce.0.1,
                                unit_to_produce.0.2,
                                unit_to_produce.0.3,
                                unit_to_produce.0.4,
                            )){
                                if unit_to_produce.0.5 == 0 {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(new_unit);
                                    }
                                } else {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(new_unit);
                                    }
                                }
                            }

                            if matches!(network_status.0, NetworkStatuses::Host) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                        unit_data: (
                                            *team_queue.0,
                                            (
                                                battalion_type,
                                                *unit_to_produce.0,
                                                unit_to_produce.1.0.clone(),
                                            ),
                                        ),
                                        position: point,
                                        server_entity: new_unit,
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

            for unit_to_produce in team_queue.1.shock_infantry_queue.iter() {
                let mut is_produced = false;
                while is_produced == false {
                    if let Some(infantry_producer) = infantry_producers_q_cycle.next(){
                        if *team_queue.0 == infantry_producer.3.team {
                            is_produced = true;
                        } else {
                            continue;
                        }

                        if let Some(bundle) = infantry_producer.1.available_to_build.get(&unit_to_produce.1.0) {
                            let mut new_unit= Entity::PLACEHOLDER;

                            let battalion_type;
                            let unit_type;

                            let point = infantry_producer.2.transform_point(infantry_producer.1.spawn_point);
                            let tile = (i32::MAX, i32::MAX);

                            match &bundle.0 {
                                UnitBundles::Soldier(b) => {
                                    battalion_type = CompanyTypes::Regular;
                                    unit_type = b.combat_component.unit_type.clone();

                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Shock;
                                    unit_type = b.combat_component.unit_type.clone();

                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                _ => {
                                    battalion_type = CompanyTypes::None;
                                    unit_type = UnitTypes::None;
                                },
                            }
        
                            if let Some(platoon) = armies.0.get_mut(team_queue.0).unwrap().shock_squads.get_mut(&(
                                unit_to_produce.0.0,
                                unit_to_produce.0.1,
                                unit_to_produce.0.2,
                                unit_to_produce.0.3,
                                unit_to_produce.0.4,
                            )){
                                if unit_to_produce.0.5 == 0 {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.0.insert(new_unit);
                                    }
                                } else {
                                    if new_unit != Entity::PLACEHOLDER {
                                        let _ = platoon.0.0.1.insert(new_unit);
                                    }
                                }
                            }

                            if matches!(network_status.0, NetworkStatuses::Host) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                        unit_data: (
                                            *team_queue.0,
                                            (
                                                battalion_type,
                                                *unit_to_produce.0,
                                                unit_to_produce.1.0.clone(),
                                            ),
                                        ),
                                        position: point,
                                        server_entity: new_unit,
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
    
            for unit_to_produce in team_queue.1.vehicles_queue.iter() {
                let mut is_produced = false;
                while is_produced == false {
                    if let Some(vehicles_producer) = vehicles_producers_q_cycle.next(){
                        if *team_queue.0 == vehicles_producer.3.team {
                            is_produced = true;
                        } else {
                            continue;
                        }

                        if let Some(bundle) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0) {
                            let mut new_unit = Entity::PLACEHOLDER;

                            let battalion_type;
                            let unit_type;

                            let point = vehicles_producer.2.transform_point(vehicles_producer.1.spawn_point);
                            let tile = (i32::MAX, i32::MAX);
        
                            match &bundle.0 {
                                UnitBundles::Soldier(b) => {
                                    battalion_type = CompanyTypes::Regular;
                                    unit_type = b.combat_component.unit_type.clone();
                                    
                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Shock;
                                    unit_type = b.combat_component.unit_type.clone();

                                    new_unit = commands.spawn((
                                        SceneBundle{
                                            scene: b.scene.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                    battalion_type = CompanyTypes::Armored;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material_turret;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_turret.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_turret.mesh.id(), *team_queue.0), material_turret.clone());
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

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model_hull.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model_hull.mesh.id(), *team_queue.0), material_hull.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model_hull.mesh.clone(),
                                            material: material_hull.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                _ => {
                                    battalion_type = CompanyTypes::None;
                                    unit_type = UnitTypes::None;
                                },
                            }
        
                            if let Some(platoon) = armies.0.get_mut(team_queue.0).unwrap().armored_squads.get_mut(&(
                                unit_to_produce.0.0,
                                unit_to_produce.0.1,
                                unit_to_produce.0.2,
                                unit_to_produce.0.3,
                                unit_to_produce.0.4,
                            )){
                                if new_unit != Entity::PLACEHOLDER {
                                let _ = platoon.0.0.insert(new_unit);
                                }
                            }

                            if matches!(network_status.0, NetworkStatuses::Host) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                        unit_data: (
                                            *team_queue.0,
                                            (
                                                battalion_type,
                                                *unit_to_produce.0,
                                                unit_to_produce.1.0.clone(),
                                            ),
                                        ),
                                        position: point,
                                        server_entity: new_unit,
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
    
            for unit_to_produce in team_queue.1.artillery_queue.iter() {      
                let mut is_produced = false;
                while is_produced == false {
                    if let Some(vehicles_producer) = vehicles_producers_q_cycle.next(){
                        if *team_queue.0 == vehicles_producer.3.team {
                            is_produced = true;
                        } else {
                            continue;
                        }

                        if let Some(bundle) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0) {
                            let mut new_unit = Entity::PLACEHOLDER;

                            let battalion_type;
                            let unit_type;

                            let point = vehicles_producer.2.transform_point(vehicles_producer.1.spawn_point);
                            let tile = (i32::MAX, i32::MAX);
        
                            match &bundle.0 {
                                UnitBundles::Artillery(b) => {
                                    battalion_type = CompanyTypes::Artillery;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model.mesh.id(), *team_queue.0), material.clone());
                                    }

                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model.mesh.clone(),
                                            material: material.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                _ => {
                                    battalion_type = CompanyTypes::None;
                                    unit_type = UnitTypes::None;
                                },
                            }
        
                            if let Some(artillery) = armies.0.get_mut(team_queue.0).unwrap().artillery_units.0.get_mut(&unit_to_produce.0.6) {
                                if new_unit != Entity::PLACEHOLDER {
                                    artillery.0.0 = Some(new_unit);
                                }
                            }

                            if matches!(network_status.0, NetworkStatuses::Host) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                        unit_data: (
                                            *team_queue.0,
                                            (
                                                battalion_type,
                                                *unit_to_produce.0,
                                                unit_to_produce.1.0.clone(),
                                            ),
                                        ),
                                        position: point,
                                        server_entity: new_unit,
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
    
            for unit_to_produce in team_queue.1.engineers_queue.iter() {
                let mut is_produced = false;
                while is_produced == false {
                    if let Some(vehicles_producer) = vehicles_producers_q_cycle.next(){
                        if *team_queue.0 == vehicles_producer.3.team {
                            is_produced = true;
                        } else {
                            continue;
                        }

                        if let Some(bundle) = vehicles_producer.1.available_to_build.get(&unit_to_produce.1.0) {
                            let mut new_unit = Entity::PLACEHOLDER;

                            let battalion_type;
                            let unit_type;

                            let point = vehicles_producer.2.transform_point(vehicles_producer.1.spawn_point);
                            let tile = (i32::MAX, i32::MAX);
        
                            match &bundle.0 {
                                UnitBundles::Engineer(b) => {
                                    battalion_type = CompanyTypes::Engineer;
                                    unit_type = b.combat_component.unit_type.clone();

                                    let material;

                                    if let Some(mat) = instanced_materials.team_materials.get(&(b.model.mesh.id(), *team_queue.0)) {
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

                                        instanced_materials.team_materials.insert((b.model.mesh.id(), *team_queue.0), material.clone());
                                    }
                                    
                                    new_unit = commands.spawn((
                                        MaterialMeshBundle{
                                            mesh: b.model.mesh.clone(),
                                            material: material.clone(),
                                            transform: Transform::from_translation(point),
                                            ..default()
                                        },
                                        b.unit_component.clone(),
                                        CombatComponent {
                                            team: *team_queue.0,
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
                                                    battalion_type,
                                                    *unit_to_produce.0,
                                                    unit_to_produce.1.0.clone(),
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
                                },
                                _ => {
                                    battalion_type = CompanyTypes::None;
                                    unit_type = UnitTypes::None;
                                },
                            }
        
                            if let Some(engineer) = armies.0.get_mut(team_queue.0).unwrap().engineers.get_mut(&unit_to_produce.0.6) {
                                if new_unit != Entity::PLACEHOLDER {
                                    engineer.0.0 = Some(new_unit);
                                }
                            }

                            if matches!(network_status.0, NetworkStatuses::Host) {
                                let mut channel_id = 60;
                                while channel_id <= 89 {
                                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitSpawned {
                                        unit_data: (
                                            *team_queue.0,
                                            (
                                                battalion_type,
                                                *unit_to_produce.0,
                                                unit_to_produce.1.0.clone(),
                                            ),
                                        ),
                                        position: point,
                                        server_entity: new_unit,
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

            team_queue.1.regular_infantry_queue.clear();
            team_queue.1.shock_infantry_queue.clear();
            team_queue.1.vehicles_queue.clear();
            team_queue.1.artillery_queue.clear();
            team_queue.1.engineers_queue.clear();
        }

        game_stage.0 = GameStages::GameStarted;

        for production_state in production_states.is_allowed.iter_mut() {
            *production_state.1 = true;
        }
    }
}

pub fn homing_projectiles_moving_system(
    mut homing_projectiles_q: Query<(Entity, &mut Transform, &mut HomingProjectile), Without<CombatComponent>>,
    units_q: Query<&Transform, With<CombatComponent>>,
    mut commands: Commands,
    mut event_writer:(
        //EventWriter<UnsentServerMessage>,
        EventWriter<ExplosionEvent>,
    ),
    time: Res<Time>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for mut projectile in homing_projectiles_q.iter_mut(){
        if let Ok(unit) = units_q.get(projectile.2.target_entity){
            let target_velocity = (unit.translation - projectile.2.targets_last_position) / time.delta_seconds();
            let intercept_time = predict_intercept_time(
                projectile.1.translation,
                projectile.2.speed,
                unit.translation,
                target_velocity,
                projectile.2.max_prediction_caluclation_iterations,
                projectile.2.prediction_tolerance,
            );

            let predicted_target_position = unit.translation + target_velocity * intercept_time;
            let direction = (predicted_target_position - projectile.1.translation).normalize();

            projectile.1.translation += direction * projectile.2.speed * time.delta_seconds();

            projectile.1.look_at(unit.translation, Vec3::Y);

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 0;
                while channel_id <= 29 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityMoved{
                        server_entity: projectile.0,
                        new_position: projectile.1.translation,
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }

            if projectile.1.translation.distance(unit.translation) <= projectile.2.hit_check_factor {
                commands.entity(projectile.0).despawn_recursive();
                event_writer.0.send(ExplosionEvent((unit.translation, projectile.2.direct_damage, projectile.2.splash_damage)));

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = server
                        .endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                            server_entity: projectile.0,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }

                    channel_id = 30;

                    while channel_id <= 59 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ExplosionOccured {
                            position: unit.translation,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            projectile.2.targets_last_position = unit.translation;
        } else {
            commands.entity(projectile.0).despawn_recursive();

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 60;
                while channel_id <= 89 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnspecifiedEntityRemoved {
                        server_entity: projectile.0,
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

pub fn predict_intercept_time(
    projectile_position: Vec3,
    projectile_speed: f32,
    target_position: Vec3,
    target_velocity: Vec3,
    max_iterations: usize,
    tolerance: f32,
) -> f32 {
    let mut t = 0.0;

    for _ in 0..max_iterations {
        let future_target_position = target_position + target_velocity * t;

        let distance_to_intercept = (future_target_position - projectile_position).length();

        let error = (distance_to_intercept - projectile_speed * t).abs();

        if error < tolerance {
            return t;
        }

        t = distance_to_intercept / projectile_speed;
    }

    t
}

pub fn explosion_processing_system (
    mut event_reader: EventReader<ExplosionEvent>,
    mut tile_map: ResMut<UnitsTileMap>,
    mut units_q: Query<(&mut CombatComponent, &Transform, Option<&Covered>, Option<&CoverComponent>, Option<&InfantryTransport>, Entity), With<CombatComponent>>,
    assets: Res<AttackVisualisationAssets>,
    mut event_writer: (
        //EventWriter<UnsentServerMessage>,
        EventWriter<UnitDeathEvent>,
    ),
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut commands: Commands,
){
    for event in event_reader.read() {
        commands.spawn(MaterialMeshBundle{
            mesh: assets.explosion_regular.1[0].clone(),
            material: assets.explosion_regular.0.clone(),
            transform: Transform::from_translation(event.0.0),
            ..default()
        })
        .try_insert(ExplosionComponent((0, 0)))
        .try_insert(AudioBundle{
            source: assets.explosion_small_sound.clone(),
            settings: PlaybackSettings{
                mode: PlaybackMode::Remove,
                volume: Volume::new(100.),
                speed: 1.,
                paused: false,
                spatial: true,
                spatial_scale: None,
            },
        });

        if matches!(network_status.0, NetworkStatuses::Client) {continue;}

        let top_right_tile = (
            ((event.0.0.x + event.0.2.0) / TILE_SIZE) as i32,
            ((event.0.0.z + event.0.2.0) / TILE_SIZE) as i32
        );
        let bottom_left_tile = (
            ((event.0.0.x - event.0.2.0) / TILE_SIZE) as i32,
            ((event.0.0.z - event.0.2.0) / TILE_SIZE) as i32
        );
        let mut tile_to_scan = bottom_left_tile;
        let rows = top_right_tile.1 - bottom_left_tile.1;
        let columns = top_right_tile.0 - bottom_left_tile.0;

        let mut extra_units_to_kill: Vec<Entity> = Vec::new();

        let mut nearest_entity = (Entity::PLACEHOLDER, f32::INFINITY);
        for _row in 0..rows + 1 {
            for _column in 0..columns + 1 {
                for team_tile_map in tile_map.tiles.iter_mut() {
                    for (unit_entity, (unit_position, unit_type)) in team_tile_map.1.entry(tile_to_scan)
                    .or_insert_with(HashMap::new) {
                        if let Ok(mut unit) = units_q.get_mut(*unit_entity) {
                            let distance_to_unit = event.0.0.distance(unit.1.translation);
                            if distance_to_unit <= event.0.2.0 {
                                let mut current_damage = event.0.2.1;
    
                                match event.0.2.2 {
                                    DamageTypes::AntiInfantry => {
                                        match unit.0.unit_type {
                                            UnitTypes::Infantry => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::LightVehicle => {
                                                current_damage /= 5;
                                            },
                                            UnitTypes::HeavyVehicle => {
                                                current_damage /= 10;
                                            },
                                            UnitTypes::Building => {
                                                current_damage /= 10;
                                            },
                                            UnitTypes::None => {},
                                        }
                                    },
                                    DamageTypes::AntiTank => {
                                        match unit.0.unit_type {
                                            UnitTypes::Infantry => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::LightVehicle => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::HeavyVehicle => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::Building => {
                                                current_damage /= 2;
                                            },
                                            UnitTypes::None => {},
                                        }
                                    },
                                    DamageTypes::AntiBuilding => {
                                        match unit.0.unit_type {
                                            UnitTypes::Infantry => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::LightVehicle => {
                                                current_damage /= 2;
                                            },
                                            UnitTypes::HeavyVehicle => {
                                                current_damage /= 3;
                                            },
                                            UnitTypes::Building => {
                                                current_damage *= 1;
                                            },
                                            UnitTypes::None => {},
                                        }
                                    },
                                    DamageTypes::Universal => {},
                                }
    
                                let mut cover_efficiency = 1.;
                                let mut cover_entity = None;
                                if let Some(cover) = unit.2 {
                                    cover_efficiency = cover.cover_efficiency;
                                    cover_entity = Some(cover.cover_entity);
                                }
    
                                unit.0.current_health -= (current_damage as f32 * cover_efficiency) as i32;
    
                                if matches!(network_status.0, NetworkStatuses::Host) {
                                    let mut channel_id = 00;
                                    while channel_id <= 59 {
                                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitDamaged {
                                            server_entity: *unit_entity,
                                            damage: current_damage,
                                        }){
                                            channel_id += 1;
                                        } else {
                                            break;
                                        }
                                    }
                                }
    
                                if unit.0.current_health <= 0 {
                                    event_writer.0.send(UnitDeathEvent { dead_unit_data:
                                        (
                                            unit.0.team,
                                            unit.0.unit_data.clone(),
                                            cover_entity,
                                            *unit_entity,
                                            *unit.1,
                                            true,
                                        )
                                    });

                                    if let Some(cover) = unit.3 {
                                        for covered_unit in cover.units_inside.iter() {
                                            extra_units_to_kill.push(*covered_unit);
                                        }
                                    }

                                    if let Some(transport) = unit.4 {
                                        for unit in transport.units_inside.iter() {
                                            extra_units_to_kill.push(*unit);
                                        }
                                    }
                                } else {
                                    if distance_to_unit < nearest_entity.1 {
                                        nearest_entity.0 = *unit_entity;
                                        nearest_entity.1 = distance_to_unit;
                                    }
                                }
                            }
                        }
                    }
                }

                tile_to_scan.0 += 1;
            }

            tile_to_scan.1 += 1;
            tile_to_scan.0 -= columns + 1;
        }

        if nearest_entity.0 != Entity::PLACEHOLDER && nearest_entity.1 <= 1. {
            if let Ok(mut unit) = units_q.get_mut(nearest_entity.0) {
                let mut current_damage = event.0.1.0;

                match event.0.1.1 {
                    DamageTypes::AntiInfantry => {
                        match unit.0.unit_type {
                            UnitTypes::Infantry => {
                                current_damage *= 1;
                            },
                            UnitTypes::LightVehicle => {
                                current_damage /= 5;
                            },
                            UnitTypes::HeavyVehicle => {
                                current_damage /= 10;
                            },
                            UnitTypes::Building => {
                                current_damage /= 10;
                            },
                            UnitTypes::None => {},
                        }
                    },
                    DamageTypes::AntiTank => {
                        match unit.0.unit_type {
                            UnitTypes::Infantry => {
                                current_damage *= 1;
                            },
                            UnitTypes::LightVehicle => {
                                current_damage *= 1;
                            },
                            UnitTypes::HeavyVehicle => {
                                current_damage *= 1;
                            },
                            UnitTypes::Building => {
                                current_damage /= 2;
                            },
                            UnitTypes::None => {},
                        }
                    },
                    DamageTypes::AntiBuilding => {
                        match unit.0.unit_type {
                            UnitTypes::Infantry => {
                                current_damage *= 1;
                            },
                            UnitTypes::LightVehicle => {
                                current_damage /= 2;
                            },
                            UnitTypes::HeavyVehicle => {
                                current_damage /= 3;
                            },
                            UnitTypes::Building => {
                                current_damage *= 1;
                            },
                            UnitTypes::None => {},
                        }
                    },
                    DamageTypes::Universal => {},
                }

                let mut cover_efficiency = 1.;
                let mut cover_entity = None;
                if let Some(cover) = unit.2 {
                    cover_efficiency = cover.cover_efficiency;
                    cover_entity = Some(cover.cover_entity);
                }

                unit.0.current_health -= (current_damage as f32 * cover_efficiency) as i32;

                if matches!(network_status.0, NetworkStatuses::Host) {
                    let mut channel_id = 00;
                    while channel_id <= 59 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitDamaged {
                            server_entity: nearest_entity.0,
                            damage: current_damage,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }
                }

                if unit.0.current_health <= 0 {
                    event_writer.0.send(UnitDeathEvent { dead_unit_data:
                        (
                            unit.0.team,
                            unit.0.unit_data.clone(),
                            cover_entity,
                            nearest_entity.0,
                            *unit.1,
                            true,
                        )
                    });

                    if let Some(cover) = unit.3 {
                        for covered_unit in cover.units_inside.iter() {
                            extra_units_to_kill.push(*covered_unit);
                        }
                    }

                    if let Some(transport) = unit.4 {
                        for unit in transport.units_inside.iter() {
                            extra_units_to_kill.push(*unit);
                        }
                    }
                }
            }
        }

        for extra_unit_to_kill in extra_units_to_kill.iter() {
            if let Ok(unit) = units_q.get(*extra_unit_to_kill) {
                let mut cover_entity: Option<Entity> = None;

                if let Some(cover) = unit.2 {
                    cover_entity = Some(cover.cover_entity);
                }

                event_writer.0.send(UnitDeathEvent { dead_unit_data:
                    (
                        unit.0.team,
                        unit.0.unit_data.clone(),
                        cover_entity,
                        unit.5,
                        *unit.1,
                        true,
                    )
                });
            }
        }
    }
}

#[derive(Resource)]
pub struct FogOfWarTexture {
    pub handle: Handle<Image>,
}

pub fn update_fog_of_war(
    mut images: ResMut<Assets<Image>>,
    fog_texture: Res<FogOfWarTexture>,
    units_q: Query<(Entity, &Transform, Option<&CombatComponent>, Option<&SettlementComponent>), Or<(With<CombatComponent>, With<SettlementComponent>)>>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
    game_stage: Res<GameStage>,
    time: Res<Time>,
    mut time_elapsed: Local<u128>,
) {
    *time_elapsed += time.delta().as_millis();

    if *time_elapsed < 500 {
        return;
    }

    *time_elapsed = 0;

    if !matches!(game_stage.0, GameStages::GameStarted) {
        return;
    }
    
    let image = match images.get(&fog_texture.handle) {
        Some(img) => img,
        None => return,
    };

    let mut data = image.data.clone();

    for byte in data.iter_mut() {
        *byte = 0;
    }

    let texture_size = FOG_TEXTURE_SIZE;

    let world_to_texel = texture_size / WORLD_SIZE;
    let half_map = WORLD_SIZE * 0.5;

    for unit in units_q.iter() {
        if let Some(combat_component) = unit.2 {
            if combat_component.team != player_data.team || combat_component.detection_range == 0. {
                continue;
            }
            
            let pos = unit.1.translation;
            let x = ((pos.x + half_map) * world_to_texel) as i32;
            let y = ((pos.z + half_map) * world_to_texel) as i32;
            let radius = (combat_component.detection_range * world_to_texel) as i32;

            draw_circle_on_texture(&mut data, texture_size as u32, x, y, radius, 255);
        } else if let Some(settlement) = unit.3 {
            if settlement.0.team != player_data.team {
                continue;
            }
            
            let pos = unit.1.translation;
            let x = ((pos.x + half_map) * world_to_texel) as i32;
            let y = ((pos.z + half_map) * world_to_texel) as i32;
            let radius = (settlement.0.settlement_size * 1.25 * world_to_texel) as i32;

            draw_circle_on_texture(&mut data, texture_size as u32, x, y, radius, 255);
        }
    }
    for unit in units_q.iter() {
        if let Some(combat_component) = unit.2 {
            if combat_component.team == player_data.team {
                continue;
            }

            if is_position_visible(unit.1.translation, &data, texture_size as u32, WORLD_SIZE) {
                commands.entity(unit.0).try_insert(Visibility::Visible);
            } else {
                commands.entity(unit.0).try_insert(Visibility::Hidden);
            }
        }
    }

    let image_asset = images.get_mut(&fog_texture.handle).unwrap();
    image_asset.data = data;
}

fn draw_circle_on_texture (
    data: &mut Vec<u8>,
    texture_size: u32,
    center_x: i32,
    center_y: i32,
    radius: i32,
    value: u8,
) {
    let min_x = (center_x - radius).max(0);
    let max_x = (center_x + radius).min(texture_size as i32 - 1);
    let min_y = (center_y - radius).max(0);
    let max_y = (center_y + radius).min(texture_size as i32 - 1);

    for y in min_y..=max_y {
        let dy = y - center_y;
        let dy2 = dy * dy;

        let x_offset = ((radius * radius - dy2) as f32).sqrt() as i32;

        let start_x = (center_x - x_offset).max(min_x);
        let end_x = (center_x + x_offset).min(max_x);

        let base_idx = (y * texture_size as i32) as usize;
        for x in start_x..=end_x {
            let idx = base_idx + x as usize;
            if idx < data.len() {
                unsafe {
                    std::ptr::write(data.get_unchecked_mut(idx), value);
                }
            }
        }
    }
}

fn is_position_visible(
    position: Vec3,
    fog_data: &Vec<u8>,
    texture_size: u32,
    map_size: f32,
) -> bool {
    let half_map = map_size * 0.5;

    let uv_x = (position.x + half_map) / map_size;
    let uv_z = (position.z + half_map) / map_size;

    let texel_x = (uv_x * texture_size as f32) as i32;
    let texel_z = (uv_z * texture_size as f32) as i32;

    if texel_x < 0 || texel_z < 0 || texel_x >= texture_size as i32 || texel_z >= texture_size as i32 {
        return false;
    }

    let idx = (texel_z * texture_size as i32 + texel_x) as usize;

    debug_assert!(idx < fog_data.len());

    idx < fog_data.len() && fog_data[idx] > 20
}

#[derive(Component)]
pub struct BulletSprite {
    pub lifetime: u128,
    pub elapsed_time: u128,
    pub speed: f32,
    pub direction: Vec3,
}

pub fn visual_projectiles_processing_system (
    mut bullets_q: Query<(Entity, &mut BulletSprite, &mut Transform), (With<BulletSprite>, Without<CameraComponent>)>,
    camera_q: Query<&Transform, (With<CameraComponent>, Without<BulletSprite>)>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let time_form_last_check = time.delta().as_millis();
    let camera_pos = camera_q.single().translation;

    for mut bullet in bullets_q.iter_mut() {
        bullet.1.elapsed_time += time_form_last_check;

        if bullet.1.elapsed_time >= bullet.1.lifetime {
            commands.entity(bullet.0).despawn();
        } else {
            if bullet.1.direction != Vec3::ZERO {
                bullet.2.translation += bullet.1.direction * bullet.1.speed * time.delta_seconds();

                let bullet_to_camera_direction = (camera_pos - bullet.2.translation).normalize();

                bullet.2.align(
                    Dir3::Z,
                    bullet.1.direction,
                    Dir3::Y,
                    bullet_to_camera_direction
                );
            }

        }
    }
}

pub fn supplies_consumption_system (
    mut supplies_consumers_q: Query<&mut SuppliesConsumerComponent>,
    time: Res<Time>,
    mut time_elapsed: Local<u128>,
){
    *time_elapsed += time.delta().as_millis();

    if *time_elapsed >= 3000 {
        *time_elapsed = 0;
        
        for mut consumer in supplies_consumers_q.iter_mut() {
            consumer.supplies -= consumer.consume_rate;

            if consumer.supplies < 0 {
                consumer.supplies = 0;
            }
        }
    }
}

#[derive(Component, Clone)]
pub struct InfantryTransport {
    pub max_units: usize,
    pub units_inside: HashSet<Entity>,
}

#[derive(Component)]
pub struct MovingToTransport{
    pub transport_entity: Entity,
    pub transport_position: Vec3,
}

#[derive(Component)]
pub struct InTransport{
    pub transport_entity: Entity,
}

#[derive(Component)]
pub struct DisabledUnit;

pub fn transport_assignation_system(
    selected_units_q: Query<(Entity, &CombatComponent, &Transform), With<SelectedUnit>>,
    transports_q: Query<(Entity, &mut InfantryTransport, &Transform, &CombatComponent), With<InfantryTransport>>,
    player_data: Res<PlayerData>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    if mouse_buttons.just_released(MouseButton::Right){
        if !selected_units_q.is_empty() && !transports_q.is_empty() {
            if let Some(cursor_ray) = **cursor_ray {
                let hits = raycast.cast_ray(cursor_ray, &default());
        
                for hit in hits.iter() {
                    if let Ok(transport) = transports_q.get(hit.0) {
                        if transport.3.team != player_data.team || transport.1.max_units <= transport.1.units_inside.len() {
                            return;
                        }

                        let mut selected_units_iter = selected_units_q.iter();

                        let mut assigned_units: Vec<Entity> = Vec::new();
                        for _i in 0..transport.1.max_units - transport.1.units_inside.len() {
                            if let Some(unit) = selected_units_iter.next() {
                                if unit.1.unit_data.1.0 != CompanyTypes::Armored {
                                    match network_status.0 {
                                        NetworkStatuses::Client => {
                                            commands.entity(unit.0).try_insert(MovingToTransport{
                                                transport_entity: transport.0,
                                                transport_position: transport.2.translation,
                                            });

                                            unstarted_tasks.0.push((
                                                TaskPoolTypes::Manual,
                                                (
                                                    unit.2.translation,
                                                    transport.2.translation,
                                                    Some(100.),
                                                    unit.0,
                                                ),
                                            ));

                                            if let Some(server_entity) = entity_maps.client_to_server.get(&unit.0) {
                                                assigned_units.push(*server_entity);
                                            }
                                        }
                                        _ => {
                                            commands.entity(unit.0).try_insert(MovingToTransport{
                                                transport_entity: transport.0,
                                                transport_position: transport.2.translation,
                                            });

                                            unstarted_tasks.0.push((
                                                TaskPoolTypes::Manual,
                                                (
                                                    unit.2.translation,
                                                    transport.2.translation,
                                                    Some(100.),
                                                    unit.0,
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                            else {
                                break;
                            }
                        }

                        if matches!(network_status.0, NetworkStatuses::Client) {
                            if let Some(server_entity) = entity_maps.client_to_server.get(&transport.0) {
                                let mut channel_id = 30;
                                while channel_id <= 59 {
                                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::TransportAssignationRequest {
                                        units: assigned_units.clone(),
                                        transport_entity: *server_entity,
                                        transport_position: transport.2.translation,
                                    }){
                                        channel_id += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }

                        break;
                    }
                }
            }
        }
    }
}

pub fn transport_disturb_system(
    units_q: Query<(Entity, &MovingToTransport), (With<SelectedUnit>, With<MovingToTransport>)>,
    transports_q: Query<Entity, (With<InfantryTransport>, Without<MovingToTransport>, Added<NeedToMove>)>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
) {
    if mouse_buttons.just_pressed(MouseButton::Right) {
        let mut canceled_units: Vec<Entity> = Vec::new();
        for unit in units_q.iter() {
            commands.entity(unit.0).remove::<MovingToTransport>();
            commands.entity(unit.0).remove::<NeedToMove>();

            if matches!(network_status.0, NetworkStatuses::Client) {
                if let Some(client_entity) = entity_maps.client_to_server.get(&unit.0) {
                    canceled_units.push(*client_entity);
                }
            }
        }

        if !canceled_units.is_empty() {
            if matches!(network_status.0, NetworkStatuses::Client) {
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::TransportAssignationCancelRequest {
                        units: canceled_units.clone(),
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }

    let mut canceled_units: Vec<Entity> = Vec::new();
    for transport in transports_q.iter() {
        for unit in units_q.iter() {
            if unit.1.transport_entity == transport {
                commands.entity(unit.0).remove::<MovingToTransport>();
                commands.entity(unit.0).remove::<NeedToMove>();

                if matches!(network_status.0, NetworkStatuses::Host) {
                    canceled_units.push(unit.0);
                }
            }
        }
    }

    if !canceled_units.is_empty() {
        if matches!(network_status.0, NetworkStatuses::Host) {
            let mut channel_id = 30;
            while channel_id <= 59 {
                if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::TransportAssignationCanceled {
                    server_entities: canceled_units.clone(),
                }){
                    channel_id += 1;
                } else {
                    break;
                }
            }
        }
    }
}

pub fn transport_embark_system (
    mut units_q: Query<(Entity, &mut Transform, &MovingToTransport, &CombatComponent), With<MovingToTransport>>,
    mut transports_q: Query<(Entity, &Transform, &mut InfantryTransport), (With<InfantryTransport>, Without<MovingToTransport>)>,
    mut tile_map: ResMut<UnitsTileMap>,
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
) {
    *elapsed_time += time.delta().as_millis();

    if *elapsed_time > 100 {
        *elapsed_time = 0;

        let mut canceled_units: Vec<Entity> = Vec::new();
        let mut embarked_units: Vec<(Entity, (i32, i32))> = Vec::new();
        let mut transport_entity = Entity::PLACEHOLDER;
        let mut team = 1;
        for mut unit in units_q.iter_mut() {
            if let Ok(mut transport) = transports_q.get_mut(unit.2.transport_entity) {
                if unit.1.translation.distance(unit.2.transport_position) < 10. {
                    if transport.2.max_units < transport.2.units_inside.len() {
                        commands.entity(unit.0).remove::<MovingToTransport>();
                        commands.entity(unit.0).remove::<NeedToMove>();

                        canceled_units.push(unit.0);
                    } else if transport.1.translation == unit.2.transport_position {
                        commands.entity(unit.0).remove::<MovingToTransport>();
                        commands.entity(unit.0).remove::<NeedToMove>();
                        commands.entity(unit.0).try_insert(DisabledUnit);
                        commands.entity(unit.0).try_insert(InTransport{
                            transport_entity: transport.0,
                        });

                        if let Some(team_map) = tile_map.tiles.get_mut(&unit.3.team) {
                            if let Some(tile) = team_map.get_mut(&unit.3.unit_data.0) {
                                tile.remove(&unit.0);
                            }
                        }

                        unit.1.translation = Vec3::new(0., 10000., 0.);

                        transport.2.units_inside.insert(unit.0);

                        embarked_units.push((unit.0, unit.3.unit_data.0));
                        transport_entity = transport.0;
                        team = unit.3.team;
                    } else {
                        commands.entity(unit.0).remove::<MovingToTransport>();
                        commands.entity(unit.0).remove::<NeedToMove>();

                        canceled_units.push(unit.0);
                    }
                }
            }
        }

        if !canceled_units.is_empty() {
            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::TransportAssignationCanceled {
                        server_entities: canceled_units.clone(),
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        if !embarked_units.is_empty() {
            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitsEmbarked {
                        server_entities: embarked_units.clone(),
                        transport_server_entity: transport_entity,
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

pub fn transport_disembark_system(
    mut event_reader: EventReader<TransportDisembarkEvent>,
    mut transports_q: Query<(&Transform, &mut InfantryTransport, Entity), (With<InfantryTransport>, With<SelectedUnit>)>,
    mut units_q: Query<(&mut Transform, &mut UnitComponent), (With<InTransport>, Without<InfantryTransport>)>,
    mut unstarted_tasks: ResMut<UnstartedPathfindingTasksPool>,
    mut commands: Commands,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    for _event in event_reader.read() {
        match network_status.0 {
            NetworkStatuses::Client => {
                let mut transports: Vec<Entity> = Vec::new();

                for transport in transports_q.iter() {
                    if let Some(server_entity) = entity_maps.client_to_server.get(&transport.2) {
                        transports.push(*server_entity);
                    }
                }
                
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::DisembarkRequest {
                        transports: transports.clone(),
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            },
            _ => {
                for mut transport in transports_q.iter_mut() {
                    let mut disembarked_units: Vec<Entity> = Vec::new();

                    for unit_entity in transport.1.units_inside.iter() {
                        if let Ok(mut unit) = units_q.get_mut(*unit_entity) {
                            commands.entity(*unit_entity).remove::<DisabledUnit>();
                            commands.entity(*unit_entity).remove::<InTransport>();

                            unit.0.translation = transport.0.translation + Vec3::new(0., 0., 0.);

                            disembarked_units.push(*unit_entity);
                        }
                    }

                    transport.1.units_inside.clear();

                    if matches!(network_status.0, NetworkStatuses::Host) {
                        let mut channel_id = 30;
                        while channel_id <= 59 {
                            if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::UnitsDisembarked {
                                server_entities: disembarked_units.clone(),
                                transport_server_entity: transport.2,
                                transport_position: transport.0.translation,
                            }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }

                    let mut origin_position = -transport.0.forward() * 20. + transport.0.translation;

                    let mut counter = 0;
                    let mut operation_counter = 0;
                    let mut operation_number = 1;   //\/
                    let mut z_minus = 2;            //1
                    let mut x_minus = 2;            //2
                    let mut z_plus = 3;             //3
                    let mut x_plus = 3;             //4
                    let offset = 5.;

                    for unit_entity in disembarked_units.iter(){
                        if let Ok(mut unit) = units_q.get_mut(*unit_entity){
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

                            unit.1.path = Vec::new();

                            unstarted_tasks.0.push((
                                TaskPoolTypes::Manual,
                                (
                                    unit.0.translation,
                                    origin_position,
                                    Some(100.),
                                    *unit_entity,
                                ),
                            ));
            
                            counter += 1;
                        }
                    }
                }
            },
        }
    }
}

#[derive(Component)]
pub struct UnitRemains{
    pub number: i32,
}

#[derive(Resource)]
pub struct RemainsCount(pub i32);

const MAX_UNIT_REMAINS_COUNT: i32 = 500;

pub fn remains_processing_system (
    remains_q: Query<(Entity, &UnitRemains), With<UnitRemains>>,
    remains_count: Res<RemainsCount>,
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
){
    *elapsed_time += time.delta().as_millis();

    if *elapsed_time > 1000 {
        *elapsed_time = 0;

        let lower_bound = remains_count.0 - MAX_UNIT_REMAINS_COUNT;

        for remains in remains_q.iter() {
            if remains.1.number <= lower_bound {
                commands.entity(remains.0).despawn();
            }
        }
    }
}