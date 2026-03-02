use std::{fs::File, io::Write, hash::Hash, panic, time::{SystemTime, UNIX_EPOCH}};

use bevy::{core_pipeline::tonemapping::Tonemapping, diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin}, ecs::batching::BatchingStrategy, math::VectorSpace, pbr::{CascadeShadowConfigBuilder, ExtendedMaterial, NotShadowCaster, OpaqueRendererMethod}, prelude::*, render::{camera::RenderTarget, render_asset::RenderAssetUsages, render_resource::{Extent3d, Face, TextureDimension, TextureFormat, TextureUsages}, renderer::RenderDevice, view::{RenderLayers, ViewDepthTexture}}, tasks::AsyncComputeTaskPool, utils::hashbrown::{HashMap, HashSet}, window::{self, PrimaryWindow, WindowMode}};
use bevy_egui::{EguiPlugin, EguiSet};
use bevy_quinnet::{client::QuinnetClientPlugin, server::QuinnetServerPlugin};
use bevy_rapier3d::{math::{Rot, Vect}, plugin::{RapierConfiguration, TimestepMode}, prelude::{CharacterLength, Collider, ComputedColliderShape, KinematicCharacterController, NoUserData, RapierPhysicsPlugin, RigidBody}, rapier::prelude::{LockedAxes, RigidBodyBuilder, RigidBodySet, RigidBodyType}, render::RapierDebugRenderPlugin};
use components::{asset_manager::{generate_circle_segments, LevelAssets, LineData, LineHolder, TerrainMaterialExtension}, building::{self, create_ring, AllApartmentsPlaced, AllRoadsGenerated, AllSettlementsPlaced, ArtilleryBundle, AssaultBundle, BuildingsBundles, BuildingsList, CoverComponent, DeleteTemporaryObjects, EngineerBundle, HumanResourceStorageComponent, IFVBundle, InfantryBarracksBundle, InfantryProducer, LogisticHubBundle, MaterialsProductionComponent, MaterialsStorageComponent, ProducableUnits, ProductionData, ProductionQueue, ProductionQueueObject, ProductionState, ResourceMinerBundle, SelectableBuilding, SettlementComponent, SettlementObject, SettlementsLeft, SoldierBundle, SuppliesProductionComponent, TankBundle, TemporaryObject, UnactivatedBlueprints, UnitBundles, UnitProductionBuildingComponent, VehicleFactoryBundle, VehiclesProducer, ALLOWED_DISTANCE_FROM_BORDERS, CITIES_COUNT, VILLAGES_COUNT}, camera::SelectionBox, logistics::{create_plane_between_points, LogisticUnitComponent, /*RoadComponent, RoadObject*/}, network::{AllPlayersPlacedSettlementsEvent, ClientGameInitializedEvent, ClientGameStartedEvent, ClientList, EntityMaps, InsertedConnectionData, NetworkStatus, NetworkStatuses, PlayerList, UnitsToDamage, UnitsToInsertPath, UnspecifiedEntitiesToMove}, ui_manager::{settlements_stage_ui_activation, setup_ingame_ui, ArmySettingsNodes, BuildingPlacementCache, BuildingToBuildSelectedEvent, ButtonAction, CancelArtilleryTargets, ChooseCompanyTypeEvent, ChooseSquadSpecializationEvent, CompleteConstruction, ConnectToHostedGameEvent, GameStartedEvent, HostNewGameEvent, LandArmyButtonClickEvent, OpenCompanyTypesEvent, OpenBuildingsListEvent, OpenSquadSpecializationsEvent, SquadSelectionEvent, ProductionStateChanged, SetupCompanyEvent, Specializations, StartSingleplayerEvent, ToggleArtilleryDesignation, ToggleProductionEvent, UiButtonNodes}, unit::{ArmoredSquad, ArmyObject, ArtilleryOrderGiven, ArtilleryUnit, AsyncTaskPools, AttackTypes, CompanyTypes, CombatComponent, DamageTypes, DeleteAfterStart, EngineerComponent, ExplosionEvent, IsArtilleryDesignationActive, IsUnitDeselectionAllowed, LimitedHashMap, LimitedHashSet, LimitedNumber, RegularSquad, SelectableUnit, ShockSquad, SuppliesConsumerComponent, UnitComponent, UnitDeathEvent, UnitNeedsToBeUncovered, UnitTypes, START_ARMORED_SQUADS_AMOUNT, START_ARTILLERY_UNITS_COUNT, START_ENGINEERS_COUNT, START_REGULAR_SQUADS_AMOUNT, START_SHOCK_SQUADS_AMOUNT}};
use bevy_mod_raycast::prelude::*;
use oxidized_navigation_serializable::{
    debug_draw::{DrawNavMesh, DrawPath, OxidizedNavigationDebugDrawPlugin}, query::{find_path, find_polygon_path, perform_string_pulling_on_path}, tiles::{deserialize_nav_mesh_tiles, serialize_nav_mesh_tiles, NavMeshTiles}, Area, NavMesh, NavMeshAffector, NavMeshAreaType, NavMeshSettings, OxidizedNavigationPlugin
};
use bevy_tasks::{TaskPool, TaskPoolBuilder};
use bevy_egui::{egui::{self, Color32, Context, Stroke}, EguiContext};

use crate::components::{asset_manager::{AnimationComponent, BuildingsAssets, InstancedAnimations, InstancedMaterials, TeamMaterialExtension, UnitAssets}, building::{BuildingStageCache, BuildingsDeletionStates, PillboxBundle, Settlements}, ui_manager::{ActivateBlueprintsDeletionMode, ActivateBuildingsDeletionCancelationMode, ActivateBuildingsDeletionMode, ArtilleryUnitSelectedEvent, BattalionSelectionEvent, BrigadeSelectionEvent, BuildingButtonHovered, BuildingHints, ChangeTacticalSymbolsLevel, CompanySelectionEvent, DisplayedTacicalSymbolsLevel, OpenTacticalSymbolsLevels, PlatoonSelectionEvent, RebuildApartments, RegimentSelectionEvent, RegimentSwipeEvent, SwitchBuildingState, TransportDisembarkEvent, UiBlocker}, unit::{AttackAnimationTypes, FogOfWarTexture, InfantryTransport, IsUnitSelectionAllowed, RemainsCount, TILE_SIZE, UnitsTileMap, UnstartedPathfindingTasksPool}};

mod components;

fn setup_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs(),
            Err(_) => 0,
        };
        let filename = format!("crash_log_{}.txt", timestamp);

        let mut file = match File::create(&filename) {
            Ok(f) => f,
            Err(_) => {
                eprintln!("Log file creation failure: {}", filename);
                match File::create("crash_log_fallback.txt") {
                    Ok(f) => f,
                    Err(_) => {
                        eprintln!("Reserve log file creation failure!");
                        return;
                    }
                }
            }
        };

        writeln!(file, "=== Bevy Crash Log ===").ok();
        writeln!(file, "Timestamp: {}", timestamp).ok();

        let cause = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };
        writeln!(file, "Panic reason: {}", cause).ok();

        if let Some(location) = panic_info.location() {
            writeln!(file, "Location: {}:{}:{}", 
                location.file(),
                location.line(),
                location.column()
            ).ok();
        } else {
            writeln!(file, "Location: unknown").ok();
        }

        eprintln!("Crash log saved to: {}", filename);
    }));
}

const WORLD_SIZE: f32 = 3000.;

const FOG_TEXTURE_SIZE: f32 = WORLD_SIZE / 8.;

fn main() {
    setup_panic_hook();

    App::new()
    // .add_plugins(FrameTimeDiagnosticsPlugin::default())
    // .add_plugins(LogDiagnosticsPlugin::default())
    .add_plugins(DefaultPlugins
        .set(bevy_mod_raycast::low_latency_window_plugin())
        .set(WindowPlugin {
            primary_window: Some(Window {
                mode: WindowMode::BorderlessFullscreen,
                title: "RTSP".into(),
                ..default()
            }),
            ..default()
        })
    )
    .add_plugins(EguiPlugin)
    .add_plugins(CursorRayPlugin)
    .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
    .insert_resource(RapierConfiguration {
        gravity: Vect::Y * -9.81 * 1.,
        physics_pipeline_active: true,
        query_pipeline_active: true,
        timestep_mode: TimestepMode::Variable {
            max_dt: 1.0 / 30.0,
            time_scale: 1.0,
            substeps: 1,
        },
        scaled_shape_subdivision: 10,
        force_update_from_transform_changes: false,
    })
    //.add_plugins(RapierDebugRenderPlugin::default())
    .add_plugins(OxidizedNavigationDebugDrawPlugin)
    // .add_plugins(OxidizedNavigationPlugin::<Collider>::new(NavMeshSettings::from_agent_and_bounds(
    //     0.5,
    //     0.5,
    //     WORLD_SIZE / 2.,
    //     -1.,
    // )))
    .add_plugins(OxidizedNavigationPlugin::<Collider>::new(NavMeshSettings{
        cell_width: 3.,
        cell_height: 2.,
        tile_width: 8,
        world_half_extents: WORLD_SIZE / 2.,
        world_bottom_bound: -1.0,
        max_traversable_slope_radians: 10.0_f32.to_radians(),
        walkable_height: 10,
        walkable_radius: 1,
        step_height: 1,
        min_region_area: 0,
        max_region_area_to_merge_into: 100,
        max_edge_length: 10,
        max_contour_simplification_error: 0.1,
        max_tile_generation_tasks: Some(std::num::NonZeroU16::new(50).unwrap()),
    }))
    .add_plugins(QuinnetServerPlugin::default())
    .add_plugins(QuinnetClientPlugin::default())
    .add_plugins(MainMenuPlugin)
    .add_plugins(SingleplayerPlugin)
    .add_plugins(LobbyServerPlugin)
    .add_plugins(LobbyClientPlugin)
    .add_plugins(GameServerPlugin)
    .add_plugins(GameClientPlugin)
    .add_plugins(GameEndPlugin)
    .add_plugins(MaterialPlugin::<ExtendedMaterial<StandardMaterial, TerrainMaterialExtension>>::default())
    .add_plugins(MaterialPlugin::<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>::default())
    .init_state::<GameState>()
    .add_event::<components::camera::MoveOrderEvent>()
    .add_event::<components::building::ProductionButtonPressed>()
    .add_event::<LandArmyButtonClickEvent>()
    .add_event::<OpenCompanyTypesEvent>()
    .add_event::<ChooseCompanyTypeEvent>()
    .add_event::<SetupCompanyEvent>()
    .add_event::<OpenSquadSpecializationsEvent>()
    .add_event::<ChooseSquadSpecializationEvent>()
    .add_event::<ToggleProductionEvent>()
    .add_event::<UnitDeathEvent>()
    .add_event::<ProductionStateChanged>()
    .add_event::<SquadSelectionEvent>()
    .add_event::<UnitNeedsToBeUncovered>()
    .add_event::<OpenBuildingsListEvent>()
    .add_event::<BuildingToBuildSelectedEvent>()
    .add_event::<AllSettlementsPlaced>()
    .add_event::<AllApartmentsPlaced>()
    .add_event::<AllRoadsGenerated>()
    .add_event::<DeleteTemporaryObjects>()
    .add_event::<ToggleArtilleryDesignation>()
    .add_event::<CancelArtilleryTargets>()
    .add_event::<ArtilleryOrderGiven>()
    .add_event::<CompleteConstruction>()
    .add_event::<GameStartedEvent>()
    .add_event::<SetupDoneEvent>()
    .add_event::<ExplosionEvent>()
    .add_event::<StartSingleplayerEvent>()
    .add_event::<HostNewGameEvent>()
    .add_event::<ConnectToHostedGameEvent>()
    .add_event::<ClientGameInitializedEvent>()
    .add_event::<ClientGameStartedEvent>()
    .add_event::<AllPlayersPlacedSettlementsEvent>()
    .add_event::<OpenTacticalSymbolsLevels>()
    .add_event::<ChangeTacticalSymbolsLevel>()
    .add_event::<PlatoonSelectionEvent>()
    .add_event::<CompanySelectionEvent>()
    .add_event::<BattalionSelectionEvent>()
    .add_event::<RegimentSelectionEvent>()
    .add_event::<BrigadeSelectionEvent>()
    .add_event::<ActivateBlueprintsDeletionMode>()
    .add_event::<ActivateBuildingsDeletionMode>()
    .add_event::<ActivateBuildingsDeletionCancelationMode>()
    .add_event::<SwitchBuildingState>()
    .add_event::<RebuildApartments>()
    .add_event::<BuildingButtonHovered>()
    .add_event::<TransportDisembarkEvent>()
    .add_event::<ArtilleryUnitSelectedEvent>()
    .add_event::<RegimentSwipeEvent>()
    .insert_resource(PlayerData{
        team: 1,
        is_all_settlements_placed: false,
        is_ready_to_start: false,
    })
    .insert_resource(components::unit::TargetPosition{
        position: Vec3::new(0., 0., 0.),
    })
    .insert_resource(components::unit::SelectedUnits{
        platoons: HashMap::new(),
    })
    .insert_resource(components::camera::SelectionBounds{
        first_point: Vec2::ZERO,
        second_point: Vec2::ZERO,
        is_selection_active: false,
        is_selection_hidden: false,
        is_ui_hovered: false,
        min_x: 0.,
        max_x: 0.,
        min_y: 0.,
        max_y: 0.,
        first_point_world: Vec3::ZERO,
        second_point_world: Vec3::ZERO,
    })
    .insert_resource(components::camera::Formation{
        points: Vec::new(),
        is_formation_active: false,
    })
    .insert_resource(components::unit::UnitsTileMap{
        tiles: HashMap::new(),
    })
    .insert_resource(components::camera::TimerResource(Timer::from_seconds(0.5, TimerMode::Repeating)))
    .insert_resource(components::unit::AsyncPathfindingTasks{
        tasks: Vec::new(),
    })
    .insert_resource(components::building::SelectedBuildings{
        buildings: Vec::new(),
    })
    .insert_resource(components::ui_manager::UiButtonNodes {
        left_bottom_node: Entity::PLACEHOLDER,
        left_bottom_node_rows: Vec::new(),
        middle_bottom_node: Entity::PLACEHOLDER,
        middle_bottom_node_row: Entity::PLACEHOLDER,
        margin: 0.,
        button_size: 0.,
        is_left_bottom_node_visible: false,
        is_middle_bottom_node_visible: false,
        middle_upper_node: Entity::PLACEHOLDER,
        middle_upper_node_row: Entity::PLACEHOLDER,
        is_middle_upper_node_visible: false,
        middle_upper_node_width: 0.,
        symbol_level_dropdown_list: Entity::PLACEHOLDER,
        right_bottom_node: Entity::PLACEHOLDER,
        right_bottom_node_rows: Vec::new(),
        hint_node: Entity::PLACEHOLDER,
        hint_text: Entity::PLACEHOLDER,
    })
    .insert_resource(components::unit::Armies(HashMap::new()))
    .insert_resource(ArmySettingsNodes {
        land_army_settings_node: Entity::PLACEHOLDER,
        is_land_army_settings_visible: false,
        batallion_type_dropdown_lists: Vec::new(),
        platoons_row: Entity::PLACEHOLDER,
        squads_row: Entity::PLACEHOLDER,
        regiments_row: Entity::PLACEHOLDER,
        battalions_row: Entity::PLACEHOLDER,
        companies_row: Entity::PLACEHOLDER,
        land_army_settings_node_height: 0,
        land_army_settings_node_width: 0,
        company_buttons: (-1, Entity::PLACEHOLDER, Vec::new()),
        platoon_specialization_dropdown_lists: Vec::new(),
        platoon_specialization_cache: Vec::new(),
        toggle_production_button: (Entity::PLACEHOLDER, LimitedNumber::new()),
        last_battalion_button_index: -1,
        last_battalion_type_dropdown_list_index: -1,
        last_platoon_specialization_dropdown_list_index: -1,
        current_regiment: 1,
        squad_specialization_dropdown_lists: (-1, Vec::new()),
        company_type_dropdown_lists: (-1, Vec::new()),
    })
    .insert_resource(Specializations{
        regular: Vec::new(),
        shock: Vec::new(),
        armored: Vec::new(),
    })
    .insert_resource(ProductionState{
        is_allowed: HashMap::new(),
    })
    .insert_resource(ProductionQueue(HashMap::new()))
    .insert_resource(BuildingsList(
        Vec::new(),
    ))
    .insert_resource(BuildingPlacementCache {
        is_active: false,
        current_building: BuildingsBundles::None,
        current_building_y_adjustment: 0.,
        current_building_check_collider: Collider::ball(0.),
        needed_buildpower: 0,
        name: "".to_string(),
        build_distance: 0.,
        resource_cost: 0,
    })
    .insert_resource(UnactivatedBlueprints(HashMap::new()))
    .insert_resource(GameStage(GameStages::SettlementsSetup))
    .insert_resource(SettlementsLeft(Vec::new()))
    .insert_resource(IsArtilleryDesignationActive(false))
    .insert_resource(IsUnitDeselectionAllowed(true))
    .insert_resource(AsyncTaskPools{
        manual_pathfinding_pool: TaskPool::new(),
        logistic_pathfinding_pool: TaskPool::new(),
        extra_pathfinding_pool: TaskPool::new(),
    })
    .insert_resource(NetworkStatus(NetworkStatuses::SinglePlayer))
    .insert_resource(InsertedConnectionData{
        ip: "".to_string(),
        username: "".to_string(),
    })
    .insert_resource(ClientList(HashMap::new()))
    .insert_resource(PlayerList(HashMap::new()))
    .insert_resource(EntityMaps{
        server_to_client: HashMap::new(),
        client_to_server: HashMap::new(),
    })
    .insert_resource(ProducableUnits{
        barrack_producables: HashMap::new(),
        factory_producables: HashMap::new(),
    })
    .insert_resource(UnspecifiedEntitiesToMove(Vec::new()))
    .insert_resource(UnitsToDamage(Vec::new()))
    .insert_resource(UnitsToInsertPath(Vec::new()))
    .insert_resource(InstancedMaterials{
        team_materials: HashMap::new(),
        blue_solid: Handle::default(),
        red_solid: Handle::default(),
        blue_transparent: Handle::default(),
        red_transparent: Handle::default(),
        wreck_material: Handle::default(),
    })
    .insert_resource(DisplayedTacicalSymbolsLevel(1))
    .insert_resource(IsUnitSelectionAllowed(true))
    .insert_resource(BuildingsDeletionStates{
        is_blueprints_deletion_active: false,
        is_buildings_deletion_active: false,
        is_buildings_deletion_cancelation_active: false,
    })
    .insert_resource(UiBlocker{
        is_bottom_left_node_blocked: false,
        is_bottom_middle_node_blocked: false,
    })
    .insert_resource(BuildingStageCache{
        buildings: HashMap::new(),
    })
    .insert_resource(InstancedAnimations{
        running_animations: HashMap::new(),
    })
    .insert_resource(RemainsCount(0))
    .insert_resource(UnstartedPathfindingTasksPool(Vec::new()))
    .run();
}

const SUPPLIES_COLOR: Color = Color::srgba(1.0, 0.5, 0., 1.);
const HUMAN_RESOURCE_COLOR: Color = Color::srgba(0.5, 0., 1., 1.);
const MATERIALS_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 1.);

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut instanced_materials: ResMut<InstancedMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut army: ResMut<components::unit::Armies>,
    mut production_queue: ResMut<ProductionQueue>,
    mut specializations: ResMut<Specializations>,
    mut buildings_list: ResMut<BuildingsList>,
    mut settlements: ResMut<SettlementsLeft>,
    player_data: Res<PlayerData>,
    mut producable_units: ResMut<ProducableUnits>,
    buildings_assets: Res<BuildingsAssets>,
    unit_assets: Res<UnitAssets>,
    //assets: Res<components::asset_manager::AttackVisualisationAssets>,
    mut images: ResMut<Assets<Image>>,
    mut event_writer: EventWriter<SetupDoneEvent>,
){
    // commands.spawn(MaterialMeshBundle{
    //     mesh: assets.explosion_regular.1[0].clone(),
    //     material: assets.explosion_regular.0.clone(),
    //     transform: Transform::from_translation(Vec3::new(0., 5., 0.)),
    //     ..default()
    // })
    // .insert(components::asset_manager::ExplosionComponent((0, 0)));

    // commands.spawn(PbrBundle{
    //     mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(1., 10., 1.) }.mesh())),
    //     transform: Transform::from_translation(Vec3::new(0., 1., 0.)),
    //     ..default()
    // }).insert(CombatComponent{
    //     team: 2,
    //     current_health: 9999999,
    //     max_health: 99999999,
    //     unit_type: UnitTypes::Infantry,
    //     attack_type: AttackTypes::None,
    //     attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
    //     attack_frequency: 0,
    //     attack_elapsed_time: 0,
    //     detection_range: 0.,
    //     attack_range: 0.,
    //     enemies: vec![],
    //     is_static: false,
    //     unit_data: (
    //         (0, 0),
    //         (
    //             CompanyTypes::None,
    //             (0, 0, 0, 0, 0, 0, 0),
    //             "".to_string(),
    //         )
    //     ),
    // }).insert(components::unit::NeedToMove);

    let size = Extent3d {
        width: FOG_TEXTURE_SIZE as u32,
        height: FOG_TEXTURE_SIZE as u32,
        depth_or_array_layers: 1,
    };

    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[255u8; (FOG_TEXTURE_SIZE * FOG_TEXTURE_SIZE) as usize],
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );

    image.texture_descriptor.usage |= TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING;

    let handle = images.add(image);
    commands.insert_resource(FogOfWarTexture { handle: handle });

    let blue_solid = materials.add(Color::srgb(0., 0., 1.));

    let red_solid = materials.add(Color::srgb(1., 0., 0.));

    let blue_transparent = materials.add(
        StandardMaterial{
            base_color:  Color::srgba(0., 1., 1., 0.25),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }
    );

    let red_transparent = materials.add(
        StandardMaterial{
            base_color:  Color::srgba(1., 0., 0., 0.25),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }
    );

    let wreck_material = materials.add(Color::srgb(0.1, 0.1, 0.1));

    instanced_materials.blue_solid = blue_solid;
    instanced_materials.red_solid = red_solid;
    instanced_materials.blue_transparent = blue_transparent;
    instanced_materials.red_transparent = red_transparent;
    instanced_materials.wreck_material = wreck_material;

    commands.spawn(LineHolder(vec![
        LineData{
            line_start: Vec2::new(WORLD_SIZE / 2., 0.),
            line_end: Vec2::new(-WORLD_SIZE / 2., 0.),
            line_width: 10.,
            highlight_color: Vec4::new(1., 0., 0., 1.),
        },
    ]))
    .insert(DeleteAfterStart);

    if player_data.team == 1 {
        commands.spawn(LineHolder(vec![
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, -ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, -ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, -ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, -ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, -WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
        ]))
        .insert(TemporaryObject);

        let camera_position = Vec3::new(0., WORLD_SIZE / 2., -WORLD_SIZE / 2.);
        
        commands.spawn((
            Camera3dBundle {
                camera: Camera{
                    ..default()
                },
                transform: Transform::from_xyz(camera_position.x, camera_position.y, camera_position.z)
                .looking_at(Vec3::new(0., 0., 0.), Vec3::ZERO),
                ..default()
            },
            IsDefaultUiCamera))
            .insert(
                components::camera::CameraComponent{
                    speed: 100.,
                }
            ).insert(
                SpatialListener::default()
            );
    } else {
        commands.spawn(LineHolder(vec![
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS, WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
            LineData{
                line_start: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, ALLOWED_DISTANCE_FROM_BORDERS),
                line_end: Vec2::new(-WORLD_SIZE / 2. + ALLOWED_DISTANCE_FROM_BORDERS, WORLD_SIZE / 2. - ALLOWED_DISTANCE_FROM_BORDERS),
                line_width: 5.,
                highlight_color: Vec4::new(0., 1., 1., 1.),
            },
        ]))
        .insert(TemporaryObject);

        let camera_position = Vec3::new(0., WORLD_SIZE / 2., WORLD_SIZE / 2.);

        commands.spawn((
            Camera3dBundle {
                camera: Camera{
                    ..default()
                },
                transform: Transform::from_xyz(camera_position.x, camera_position.y, camera_position.z)
                .looking_at(Vec3::new(0., 0., 0.), Vec3::ZERO),
                ..default()
            },
            IsDefaultUiCamera))
            .insert(
                components::camera::CameraComponent{
                    speed: 100.,
                }
            ).insert(
                SpatialListener::default()
            );
    }

    let selection_box = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Percent(0.),
            height: Val::Percent(0.),
            ..default()
        },
        background_color: Color::srgba(0., 1., 1., 0.1).into(),
        ..default()
    };

    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 2000.0,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4, 0.0)),
        // cascade_shadow_config: CascadeShadowConfigBuilder {
        //     num_cascades: 3,
        //     minimum_distance: 0.1,
        //     maximum_distance: 60.0,
        //     first_cascade_far_bound: 15.0,
        //     overlap_proportion: 0.1,
        // }
        // .into(),
        ..default()
    });

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 200.,
        ..default()
    });

    commands.spawn(selection_box).insert(SelectionBox);

    // let floor = MaterialMeshBundle {
    //     mesh: meshes.add(Mesh::from(Plane3d::default().mesh().size(WORLD_SIZE, WORLD_SIZE))),
    //     material: materials.add(Color::srgb(0., 0.5, 0.)),
    //     ..default()
    // };

    // commands.spawn(floor)
    // .insert(Collider::cuboid(WORLD_SIZE / 2., 0.1, WORLD_SIZE / 2.))
    // .insert(NavMeshAffector);

    // let center = MaterialMeshBundle {
    //     mesh: meshes.add(Mesh::from(Cylinder{ radius: 1., half_height: 5. }.mesh())),
    //     material: materials.add(Color::srgb(1., 0., 0.)),
    //     transform: Transform::from_translation(Vec3::new(0., 2.5, 0.)),
    //     ..default()
    // };

    // commands.spawn(center)
    // .insert(Collider::cuboid(1., 5., 1.))
    // .insert(NavMeshAffector);

    production_queue.0.insert(1, ProductionQueueObject{
        regular_infantry_queue: HashMap::new(),
        shock_infantry_queue: HashMap::new(),
        vehicles_queue: HashMap::new(),
        artillery_queue: HashMap::new(),
        engineers_queue: HashMap::new(),
    });
    production_queue.0.insert(2, ProductionQueueObject{
        regular_infantry_queue: HashMap::new(),
        shock_infantry_queue: HashMap::new(),
        vehicles_queue: HashMap::new(),
        artillery_queue: HashMap::new(),
        engineers_queue: HashMap::new(),
    });

    army.0.insert(1, ArmyObject{
        regular_squads: HashMap::new(),
        shock_squads: HashMap::new(),
        armored_squads: HashMap::new(),
        artillery_units: (HashMap::new(), Entity::PLACEHOLDER),
        engineers: HashMap::new(),
    });
    army.0.insert(2, ArmyObject{
        regular_squads: HashMap::new(),
        shock_squads: HashMap::new(),
        armored_squads: HashMap::new(),
        artillery_units: (HashMap::new(), Entity::PLACEHOLDER),
        engineers: HashMap::new(),
    });

    let mut squad_id: LimitedNumber<1, 3> = LimitedNumber::new();
    let mut platoon_id: LimitedNumber<1, 3> = LimitedNumber::new();
    let mut company_id: LimitedNumber<1, 3> = LimitedNumber::new();
    let mut battalion_id: LimitedNumber<1, 3> = LimitedNumber::new();
    let mut regiment_id: LimitedNumber<1, 3> = LimitedNumber::new();
    squad_id.set_value(0);

    for _i in 0..START_REGULAR_SQUADS_AMOUNT {
        if squad_id.next() {
            if platoon_id.next() {
                if company_id.next() {
                    if battalion_id.next() {
                        regiment_id.next();
                    }
                }
            }
        }

        army.0.get_mut(&player_data.team).unwrap().regular_squads.insert((
            regiment_id.get_value(),
            battalion_id.get_value(),
            company_id.get_value(),
            platoon_id.get_value(),
            squad_id.get_value(),
        ), (RegularSquad((LimitedHashSet::new(), LimitedHashSet::new())), "atgm".to_string(), Entity::PLACEHOLDER));
    }

    for _i in 0..START_SHOCK_SQUADS_AMOUNT {
        if squad_id.next() {
            if platoon_id.next() {
                if company_id.next() {
                    if battalion_id.next() {
                        regiment_id.next();
                    }
                }
            }
        }

        army.0.get_mut(&player_data.team).unwrap().shock_squads.insert((
            regiment_id.get_value(),
            battalion_id.get_value(),
            company_id.get_value(),
            platoon_id.get_value(),
            squad_id.get_value(),
        ), (ShockSquad((LimitedHashSet::new(), LimitedHashSet::new())), "lat".to_string(), Entity::PLACEHOLDER));
    }

    for _i in 0..START_ARMORED_SQUADS_AMOUNT {
        if squad_id.next() {
            if platoon_id.next() {
                if company_id.next() {
                    if battalion_id.next() {
                        regiment_id.next();
                    }
                }
            }
        }

        army.0.get_mut(&player_data.team).unwrap().armored_squads.insert((
            regiment_id.get_value(),
            battalion_id.get_value(),
            company_id.get_value(),
            platoon_id.get_value(),
            squad_id.get_value(),
        ), (ArmoredSquad(LimitedHashSet::new()), "tank".to_string(), Entity::PLACEHOLDER));
    }

    for i in 1..START_ARTILLERY_UNITS_COUNT + 1 {
        army.0.get_mut(&player_data.team).unwrap().artillery_units.0.insert(i, ((None, "artillery".to_string()), Entity::PLACEHOLDER));
    }

    for i in 1..START_ENGINEERS_COUNT + 1 {
        army.0.get_mut(&player_data.team).unwrap().engineers.insert(i, ((None, "engineer".to_string()), Entity::PLACEHOLDER));
    }

    specializations.regular = vec![("atgm".to_string(), "ATGM".to_string()), ("sniperr".to_string(), "Sniper".to_string())];
    specializations.shock = vec![("lat".to_string(), "LAT".to_string()), ("snipers".to_string(), "Sniper".to_string())];
    specializations.armored = vec![("tank".to_string(), "Tank".to_string()), ("ifv".to_string(), "IFV".to_string())];

    let mut x = 30.;
    let mut z = 30.;

    let mut barracks_buildables: HashMap<String, (building::UnitBundles, building::ProductionData)> = HashMap::new();

    let soldier_lod = MaterialMeshBundle{
        mesh: unit_assets.infantry_simplified_mesh.clone(),
        ..default()
    };

    let tank_lod = MaterialMeshBundle{
        mesh: unit_assets.vehicle_simplified_mesh.clone(),
        ..default()
    };

    let tank_turret_lod = MaterialMeshBundle{
        ..default()
    };

    let custom_shape_infantry = Some((Collider::cuboid(0.25, 0.5, 0.25), Vec3::new(0., 0.5, 0.), Quat::IDENTITY));

    for _i in 0..1 {
        x += 15.;

        barracks_buildables.insert(
        "regular_soldier".to_string(),
            (building::UnitBundles::Soldier(SoldierBundle{
            scene: unit_assets.regular_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::Direct(30, 0.5, DamageTypes::AntiInfantry),
                attack_animation_type: AttackAnimationTypes::LowCaliber(Vec3::new(0., 1., 0.)),
                attack_frequency: 250,
                attack_elapsed_time: 250,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 90.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.regular_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 70,
                human_resource_cost: 1,
            }),
        );
        barracks_buildables.insert(
        "atgm".to_string(),
            (building::UnitBundles::Soldier(SoldierBundle{
            scene: unit_assets.atgm_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::HomingProjectile(20., 5., 5, 1., (1000, DamageTypes::AntiTank), (5., 1000, DamageTypes::AntiInfantry), Vec3::new(0., 1., 0.)),
                attack_animation_type: AttackAnimationTypes::MissileLaunch(Vec3::new(0., 1., 0.)),
                attack_frequency: 5000,
                attack_elapsed_time: 5000,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 90.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.atgm_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 200,
                human_resource_cost: 1,
            }),
        );
        barracks_buildables.insert(
        "shock_soldier".to_string(),
            (building::UnitBundles::Shock(AssaultBundle{
            scene: unit_assets.assault_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::Direct(30, 0.5, DamageTypes::AntiInfantry),
                attack_animation_type: AttackAnimationTypes::LowCaliber(Vec3::new(0., 1., 0.)),
                attack_frequency: 250,
                attack_elapsed_time: 250,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 90.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.assault_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 100,
                human_resource_cost: 1,
            }),
        );
        barracks_buildables.insert(
        "lat".to_string(),
            (building::UnitBundles::Shock(AssaultBundle{
            scene: unit_assets.rpg_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::BallisticProjectile(5., 2, 20., 5., 0., (500, DamageTypes::AntiTank), (5., 500, DamageTypes::AntiInfantry), Vec3::new(0., 1., 0.)),
                attack_animation_type: AttackAnimationTypes::MissileLaunch(Vec3::new(0., 1., 0.)),
                attack_frequency: 5000,
                attack_elapsed_time: 5000,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 90.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.rpg_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 100,
                human_resource_cost: 1,
            }),
        );

        barracks_buildables.insert(
        "sniperr".to_string(),
            (building::UnitBundles::Soldier(SoldierBundle{
            scene: unit_assets.sniper_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::Direct(150, 0.5, DamageTypes::AntiInfantry),
                attack_animation_type: AttackAnimationTypes::LowCaliber(Vec3::new(0., 1., 0.)),
                attack_frequency: 3000,
                attack_elapsed_time: 3000,
                enemies: Vec::new(),
                detection_range: 150.,
                attack_range: 125.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.sniper_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 70,
                human_resource_cost: 1,
            }),
        );

        barracks_buildables.insert(
        "snipers".to_string(),
            (building::UnitBundles::Shock(AssaultBundle{
            scene: unit_assets.sniper_soldier.0.clone(),
            lod: soldier_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 10.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 100,
                max_health: 100,
                unit_type: UnitTypes::Infantry,
                attack_type: AttackTypes::Direct(150, 0.5, DamageTypes::AntiInfantry),
                attack_animation_type: AttackAnimationTypes::LowCaliber(Vec3::new(0., 1., 0.)),
                attack_frequency: 3000,
                attack_elapsed_time: 3000,
                enemies: Vec::new(),
                detection_range: 150.,
                attack_range: 125.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                custom_shape: custom_shape_infantry.clone(),
                up: Vec3::Y,
                offset: CharacterLength::Absolute(0.1),
                slide: true,
                autostep: None,
                apply_impulse_to_dynamic_bodies: false,
                snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                ..default()
            },
            animation_component: AnimationComponent(unit_assets.sniper_soldier.1.clone()),
            material_change_marker: components::asset_manager::ChangeMaterial,
            }),
            building::ProductionData {
                time_to_produce: 5000,
                resource_cost: 70,
                human_resource_cost: 1,
            }),
        );


        producable_units.barrack_producables = barracks_buildables.clone();
        

        let building = MaterialMeshBundle {
            mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 2., 2.) }.mesh())),
            material: materials.add(Color::srgb(0., 0., 1.)),
            transform: Transform::from_translation(Vec3::new(x, 1., z)),
            ..default()
        };
    
        // let building_entity = commands.spawn(building)
        // .insert(Collider::cuboid(2., 2., 2.))
        // .insert(NavMeshAffector)
        // .insert(UnitProductionBuildingComponent {
        //     available_to_build: barracks_buildables.clone(),
        //     build_order: Vec::new(),
        //     elapsed_time: 0,
        // })
        // .insert(CombatComponent {
        //     team: 1,
        //     health: 1000,
        //     damage: 0,
        //     accuracy: 0.,
        //     enemies: Vec::new(),
        //     detection_range: 0.,
        //     tile_key: ((x / components::unit::TILE_SIZE) as i32, (z / components::unit::TILE_SIZE) as i32),
        //     is_static: true,
        //     unit_data: (UnitTypes::None, (-1, -1, -1, -1, -1, -1, -1), "".to_string()),
        // })
        // .insert(SelectableBuilding)
        // .insert(Name::new("barracks"))
        // .insert(InfantryProducer)
        // .id();
    
        // tile_map.tiles.entry(((x / components::unit::TILE_SIZE) as i32, (z / components::unit::TILE_SIZE) as i32))
        // .or_insert_with(HashMap::new).insert(building_entity, (Vec3::new(x, 0., z), 1));
    }

    let building = MaterialMeshBundle {
        mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 2., 2.) }.mesh())),
        material: materials.add(Color::srgb(0., 0., 1.)),
        transform: Transform::from_translation(Vec3::new(10., 1., 10.)),
        ..default()
    };

    // let building_entity = commands.spawn(building)
    // .insert(Collider::cuboid(2., 2., 2.))
    // .insert(NavMeshAffector)
    // .insert(Cover{
    //     cover_efficiency: 0.5,
    //     points: vec![
    //         Vec3::new(10., 0.5, 10.),
    //         Vec3::new(10., 0.5, 10.),
    //         Vec3::new(10., 0.5, 10.),
    //         Vec3::new(10., 0.5, 10.),
    //         Vec3::new(10., 0.5, 10.),
    //         ],
    //     units_inside: HashMap::new().into(),
    // })
    // .id();

    // tile_map.tiles.entry(((x / components::unit::TILE_SIZE) as i32, (z / components::unit::TILE_SIZE) as i32))
    //         .or_insert_with(HashMap::new).insert(building_entity, (Vec3::new(10., 0., 10.), 1));

    let mut vehicle_factory_buildables: HashMap<String, (building::UnitBundles, building::ProductionData)> = HashMap::new();

    vehicle_factory_buildables.insert(
        "tank".to_string(),
         (building::UnitBundles::Tank(TankBundle{
            model_hull: MaterialMeshBundle{
                mesh: unit_assets.tank.0.clone(),
                material: unit_assets.tank.2.clone(),
                ..default()
            },
            model_turret: MaterialMeshBundle{
                mesh: unit_assets.tank.1.clone(),
                material: unit_assets.tank.2.clone(),
                ..default()
            },
            lod: (tank_lod.clone(), tank_turret_lod.clone()),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 15.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::HeavyVehicle,
                attack_type: AttackTypes::BallisticProjectile(5., 2, 50., 5., 0., (1000, DamageTypes::AntiTank), (5., 500, DamageTypes::AntiInfantry), Vec3::new(0., 1., 0.)),
                attack_animation_type: AttackAnimationTypes::TankCannon(Vec3::new(0., 1., 0.)),
                attack_frequency: 5000,
                attack_elapsed_time: 5000,
                enemies: Vec::new(),
                detection_range: 150.,
                attack_range: 125.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                    custom_shape: custom_shape_infantry.clone(),
                    up: Vec3::Y,
                    offset: CharacterLength::Absolute(0.1),
                    slide: true,
                    autostep: None,
                    apply_impulse_to_dynamic_bodies: false,
                    snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                    ..default()
                },
            }),
            building::ProductionData {
                time_to_produce: 50000,
                resource_cost: 1500,
                human_resource_cost: 3,
            }),
    );

    vehicle_factory_buildables.insert(
        "ifv".to_string(),
         (building::UnitBundles::IFV(IFVBundle{
            model_hull: MaterialMeshBundle{
                mesh: unit_assets.ifv.0.clone(),
                material: unit_assets.ifv.2.clone(),
                ..default()
            },
            model_turret: MaterialMeshBundle{
                mesh: unit_assets.ifv.1.clone(),
                material: unit_assets.ifv.2.clone(),
                ..default()
            },
            lod: (tank_lod.clone(), tank_turret_lod.clone()),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 15.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::LightVehicle,
                attack_type: AttackTypes::Direct(110, 0.8, DamageTypes::AntiTank),
                attack_animation_type: AttackAnimationTypes::HighCaliber(Vec3::new(0., 1., 0.)),
                attack_frequency: 1000,
                attack_elapsed_time: 1000,
                enemies: Vec::new(),
                detection_range: 120.,
                attack_range: 100.,
                is_static: false,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            transport: InfantryTransport {
                max_units: 9,
                units_inside: HashSet::new(),
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                    custom_shape: custom_shape_infantry.clone(),
                    up: Vec3::Y,
                    offset: CharacterLength::Absolute(0.1),
                    slide: true,
                    autostep: None,
                    apply_impulse_to_dynamic_bodies: false,
                    snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                    ..default()
                },
            }),
            building::ProductionData {
                time_to_produce: 50000,
                resource_cost: 1000,
                human_resource_cost: 3,
            }),
    );

    vehicle_factory_buildables.insert(
        "artillery".to_string(),
         (building::UnitBundles::Artillery(ArtilleryBundle{
            model: MaterialMeshBundle{
                mesh: unit_assets.artillery.0.clone(),
                material: unit_assets.artillery.1.clone(),
                ..default()
            },
            lod: tank_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 15.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::LightVehicle,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::new(0., 1., 0.)),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 30.,
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
            },
            artillery_component: ArtilleryUnit{
                peak_trajectory_height: 100.,
                trajectory_points: 32,
                projectile_waypoints_check_factor: 5.,
                shell_speed: 50.,
                max_range: 600.,
                accuracy: 10.,
                reload_time: 5000,
                elapsed_reload_time: 0,
                direct_damage: (1000, DamageTypes::Universal),
                splash_damage: (10., 200, DamageTypes::AntiInfantry),
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 100,
                supplies: 100,
                consume_rate: 1,
                supply_range: 20.,
                supply_frequency: 180000,
                elapsed_time: 0,
            },
            selectable: components::unit::SelectableUnit,
            controller: KinematicCharacterController{
                    custom_shape: custom_shape_infantry.clone(),
                    up: Vec3::Y,
                    offset: CharacterLength::Absolute(0.1),
                    slide: true,
                    autostep: None,
                    apply_impulse_to_dynamic_bodies: false,
                    snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                    ..default()
                },
            }),
            building::ProductionData {
                time_to_produce: 50000,
                resource_cost: 2000,
                human_resource_cost: 3,
            }),
    );

    vehicle_factory_buildables.insert(
        "engineer".to_string(),
         (building::UnitBundles::Engineer(EngineerBundle{
            model: MaterialMeshBundle{
                mesh: unit_assets.engineer.0.clone(),
                material: unit_assets.engineer.1.clone(),
                ..default()
            },
            lod: tank_lod.clone(),
            unit_component: UnitComponent {
                path: Vec::new(),
                start_position: Vec3::ZERO,
                quantized_destination: None,
                speed: 15.,
                waypoint_radius: 0.5,
                elapsed: 0.,
                inv_duration: 0.,
                last_position: Vec3::ZERO,
                stuck_count: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
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
            },
            engineer_component: EngineerComponent {
                build_power: 10,
            },
            supplies_consumer: SuppliesConsumerComponent {
                supplies_capacity: 0,
                supplies: 0,
                consume_rate: 0,
                supply_range: 0.,
                supply_frequency: 0,
                elapsed_time: 0,
            },
            controller: KinematicCharacterController{
                    custom_shape: custom_shape_infantry.clone(),
                    up: Vec3::Y,
                    offset: CharacterLength::Absolute(0.1),
                    slide: true,
                    autostep: None,
                    apply_impulse_to_dynamic_bodies: false,
                    snap_to_ground: Some(CharacterLength::Absolute(1000.)),
                    ..default()
                },
            }),
            building::ProductionData {
                time_to_produce: 50000,
                resource_cost: 1000,
                human_resource_cost: 1,
            }),
    );


    producable_units.factory_producables = vehicle_factory_buildables.clone();

    buildings_list.0.push((
        "InfB".to_string(),
        BuildingsBundles::InfantryBarracks(InfantryBarracksBundle{
            model: MaterialMeshBundle{
                mesh: buildings_assets.barracks.0.clone(),
                material: buildings_assets.barracks.1.clone(),
                ..default()
            },
            collider: Collider::cuboid(28., 5., 34.),
            building_component: UnitProductionBuildingComponent{
                available_to_build: barracks_buildables.clone(),
                build_order: Vec::new(),
                spawn_point: Vec3::new(0., 1., 40.),
                elapsed_time: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::Building,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 0.,
                is_static: true,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            selectable: SelectableBuilding,
            producer: InfantryProducer,
            human_resource_storage: HumanResourceStorageComponent{
                human_resource_storage_capacity: 50,
                available_human_resources: 50,
                replenishment_amount: 10,
                replenishment_cooldown: 10000,
                replenishment_time_elapsed: 0,
                replenishment_local_point: Vec3::new(20., 0., 0.),
                replenishment_range: 50.,
            },
            materials_storage: MaterialsStorageComponent{
                materials_storage_capacity: 24000,
                available_resources: 24000,
                replenishment_amount: 4000,
                replenishment_cooldown: 100000,
                replenishment_time_elapsed: 0,
                replenishment_local_point: Vec3::new(20., 0., 0.),
                replenishment_range: 50.,
            },
        }),
        Collider::cuboid(32., 5., 40.),
        0.,
        200,
        30.,
        100000,
    ));

    buildings_list.0.push((
        "VehF".to_string(),
        BuildingsBundles::VehicleFactory(VehicleFactoryBundle{
            model: MaterialMeshBundle{
                mesh: buildings_assets.vehicle_factory.0.clone(),
                material: buildings_assets.vehicle_factory.1.clone(),
                ..default()
            },
            collider: Collider::cuboid(24., 5., 32.),
            building_component: UnitProductionBuildingComponent{
                available_to_build: vehicle_factory_buildables.clone(),
                build_order: Vec::new(),
                spawn_point: Vec3::new(0., 1., 40.),
                elapsed_time: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::Building,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 100.,
                attack_range: 0.,
                is_static: true,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
            selectable: SelectableBuilding,
            producer: VehiclesProducer,
            human_resource_storage: HumanResourceStorageComponent{
                human_resource_storage_capacity: 50,
                available_human_resources: 50,
                replenishment_amount: 10,
                replenishment_cooldown: 10000,
                replenishment_time_elapsed: 0,
                replenishment_local_point: Vec3::new(20., 0., 0.),
                replenishment_range: 50.,
            },
            materials_storage: MaterialsStorageComponent{
                materials_storage_capacity: 24000,
                available_resources: 24000,
                replenishment_amount: 4000,
                replenishment_cooldown: 100000,
                replenishment_time_elapsed: 0,
                replenishment_local_point: Vec3::new(20., 0., 0.),
                replenishment_range: 50.,
            },
        }),
        Collider::cuboid(30., 5., 50.),
        0.,
        200,
        30.,
        100000,
    ));

    buildings_list.0.push((
        "LogH".to_string(),
        BuildingsBundles::LogisticHub(LogisticHubBundle{
            model: MaterialMeshBundle{
                mesh: buildings_assets.logistic_hub.0.clone(),
                material: buildings_assets.logistic_hub.1.clone(),
                ..default()
            },
            collider: Collider::cuboid(3., 5., 9.),
            building_component: SuppliesProductionComponent{
                supplies_storage_capacity: 6600,
                available_supplies: 6600,
                supplies_production: (2200, ProductionData{
                    time_to_produce: 3000,
                    resource_cost: 1200,
                    human_resource_cost: 0,
                }),
                production_local_point: Vec3::new(20., 0., 0.),
                elapsed_production_time: 0,
                supply_cooldown: 6000,
                elapsed_cooldown_time: 0,
            },
            storage: MaterialsStorageComponent{
                materials_storage_capacity: 24000,
                available_resources: 24000,
                replenishment_amount: 4000,
                replenishment_cooldown: 10000,
                replenishment_time_elapsed: 0,
                replenishment_local_point: Vec3::new(20., 0., 0.),
                replenishment_range: 50.,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::Building,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 50.,
                attack_range: 0.,
                is_static: true,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
        }),
        Collider::cuboid(5., 5., 15.),
        0.,
        200,
        10.,
        10000,
    ));

    buildings_list.0.push((
        "ResM".to_string(),
        BuildingsBundles::ResourceMiner(ResourceMinerBundle{
            model: MaterialMeshBundle{
                mesh: buildings_assets.resource_extractor.0.clone(),
                material: buildings_assets.resource_extractor.1.clone(),
                ..default()
            },
            collider: Collider::cuboid(5., 5., 15.),
            building_component: MaterialsProductionComponent{
                materials_storage_capacity: 100000,
                available_materials: 0,
                materials_production_rate: 6000,
                materials_production_speed: 10000,
                production_local_point: Vec3::new(30., 0., 0.),
                elapsed_time: 0,
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::Building,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 50.,
                attack_range: 0.,
                is_static: true,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
        }),
        Collider::cuboid(20., 5., 20.),
        0.,
        200,
        20.,
        10000,
    ));

        buildings_list.0.push((
        "PillB".to_string(),
        BuildingsBundles::Pillbox(PillboxBundle{
            model: MaterialMeshBundle{
                mesh: buildings_assets.pillbox.0.clone(),
                material: buildings_assets.pillbox.1.clone(),
                ..default()
            },
            collider: Collider::cuboid(5., 5., 15.),
            building_component: CoverComponent {
                cover_efficiency: 0.5,
                points: vec![
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                    Vec3::new(0., 0., 0.),
                ],
                units_inside: HashSet::new(),
            },
            combat_component: CombatComponent {
                team: 1,
                current_health: 1000,
                max_health: 1000,
                unit_type: UnitTypes::Building,
                attack_type: AttackTypes::None,
                attack_animation_type: AttackAnimationTypes::None(Vec3::ZERO),
                attack_frequency: 0,
                attack_elapsed_time: 0,
                enemies: Vec::new(),
                detection_range: 50.,
                attack_range: 0.,
                is_static: true,
                unit_data: (
                    (0, 0),
                    (
                        CompanyTypes::None,
                        (-1, -1, -1, -1, -1, -1, -1),
                        "".to_string(),
                    ),
                ),
            },
        }),
        Collider::cuboid(20., 5., 20.),
        0.,
        200,
        30.,
        10000,
    ));

    x = -5.;
    z = -20.;

    let mut buildings_stage_cache: HashMap<String, (i32, bool)> = HashMap::new();
    let mut bulding_hints: HashMap<String, String> = HashMap::new();

    for building in buildings_list.0.iter() {
        let number: i32;
        let is_req: bool;

        let hint: String;

        match building.0.as_str() {
            "InfB" => {
                number = 4;
                is_req = true;

                hint = "Infantry barracks | cost: 100 000 materials\nProduces infantry units.".to_string();
            },
            "VehF" => {
                number = 4;
                is_req = true;

                hint = "Vehicle Factory | cost: 100 000 materials\nProduces armored vehicles.".to_string();
            },
            "LogH" => {
                number = 9;
                is_req = true;

                hint = "Logistic Hub | cost: 10 000 materials\nProvides supplies to the units. You need to have at least 9 of these, having more is not necessary.".to_string();
            },
            "ResM" => {
                number = 6;
                is_req = true;

                hint = "Materials extractor | cost: 10 000 materials\nProduces materials. Materials are spent on the production of units and the construction of buildings.\nCan only be placed inside green circled zones!".to_string();
            },
            "PillB" => {
                number = 20;
                is_req = false;

                hint = "Pillbox | cost: 10 000 materials\nDefensive structure. You can place a squad here.".to_string();
            },
            _ => {
                number = 0;
                is_req = false;

                hint = "".to_string();
            },
        }

        buildings_stage_cache.insert(building.0.clone(), (number, is_req));
        bulding_hints.insert(building.0.clone(), hint);
    }

    commands.insert_resource(BuildingStageCache{
        buildings: buildings_stage_cache,
    });

    commands.insert_resource(BuildingHints(bulding_hints));

    // for _i in 0..3 {
    //     x += 5.;

    //     let engineer = MaterialMeshBundle {
    //         mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(1., 0.5, 0.5) }.mesh())),
    //         material: materials.add(Color::srgb(0., 0., 1.)),
    //         transform: Transform::from_translation(Vec3::new(x, 0.25, z)),
    //         ..default()
    //     };

    //     commands.spawn(engineer)
    //     .insert(UnitComponent{
    //         path: Vec::new(),
    //         speed: 15.,
    //     })
    //     .insert(EngineerComponent{
    //         build_power: 10,
    //     });
    // }

    // x = 25.;
    // z = 50.;

    // let mut x1 = x + 75.;
    // let mut z1 = z + 20.;

    // let y = 0.01;

    // let first_road_entity = commands.spawn(MaterialMeshBundle{
    //     mesh: meshes.add(create_plane_between_points(
    //         &Transform::from_translation(Vec3::new(x, y, z)),
    //         Vec3::new(x, y, z),
    //         Vec3::new(x1, y, z1),
    //         2.
    //     )),
    //     material: materials.add(Color::srgb(0.3, 0.3, 0.3)).into(),
    //     transform: Transform::from_translation(Vec3::new(x, y, z)),
    //     ..default()
    // }).id();

    // let first_road_object = RoadObject{
    //     road_ends: vec![Vec3::new(x, y, z), Vec3::new(x1, y, z1)],
    //     road_center: (Vec3::new(x, y, z) + Vec3::new(x1, y, z1)) / 2.,
    // };

    // x = x1;
    // z = z1;
    // x1 = x + 50.;
    // z1 = z + 30.;

    // let second_road_entity = commands.spawn(MaterialMeshBundle{
    //     mesh: meshes.add(create_plane_between_points(
    //         &Transform::from_translation(Vec3::new(x, y, z)),
    //         Vec3::new(x, y, z),
    //         Vec3::new(x1, y, z1),
    //         2.
    //     )),
    //     material: materials.add(Color::srgb(0.3, 0.3, 0.3)).into(),
    //     transform: Transform::from_translation(Vec3::new(x, y, z)),
    //     ..default()
    // }).id();

    // let second_road_object = RoadObject{
    //     road_ends: vec![Vec3::new(x, y, z), Vec3::new(x1, y, z1)],
    //     road_center: (Vec3::new(x, y, z) + Vec3::new(x1, y, z1)) / 2.,
    // };

    // x = x1;
    // z = z1;
    // x1 = x + 50.;
    // z1 = z + 70.;

    // let third_road_entity = commands.spawn(MaterialMeshBundle{
    //     mesh: meshes.add(create_plane_between_points(
    //         &Transform::from_translation(Vec3::new(x, y, z)),
    //         Vec3::new(x, y, z),
    //         Vec3::new(x1, y, z1),
    //         2.
    //     )),
    //     material: materials.add(Color::srgb(0.3, 0.3, 0.3)).into(),
    //     transform: Transform::from_translation(Vec3::new(x, y, z)),
    //     ..default()
    // }).id();

    // let third_road_object = RoadObject{
    //     road_ends: vec![Vec3::new(x, y, z), Vec3::new(x1, y, z1)],
    //     road_center: (Vec3::new(x, y, z) + Vec3::new(x1, y, z1)) / 2.,
    // };

    // x1 += 20.;
    // z1 -= 50.;

    // let fourth_road_entity = commands.spawn(MaterialMeshBundle{
    //     mesh: meshes.add(create_plane_between_points(
    //         &Transform::from_translation(Vec3::new(x, y, z)),
    //         Vec3::new(x, y, z),
    //         Vec3::new(x1, y, z1),
    //         2.
    //     )),
    //     material: materials.add(Color::srgb(0.3, 0.3, 0.3)).into(),
    //     transform: Transform::from_translation(Vec3::new(x, y, z)),
    //     ..default()
    // }).id();

    // let fourth_road_object = RoadObject{
    //     road_ends: vec![Vec3::new(x, y, z), Vec3::new(x1, y, z1)],
    //     road_center: (Vec3::new(x, y, z) + Vec3::new(x1, y, z1)) / 2.,
    // };

    // commands.entity(first_road_entity).insert(RoadComponent((
    //     first_road_object,
    //     vec![second_road_entity],
    // )));

    // commands.entity(second_road_entity).insert(RoadComponent((
    //     second_road_object,
    //     vec![third_road_entity, fourth_road_entity],
    // )));

    // commands.entity(third_road_entity).insert(RoadComponent((
    //     third_road_object,
    //     Vec::new(),
    // )));

    // commands.entity(fourth_road_entity).insert(RoadComponent((
    //     fourth_road_object,
    //     Vec::new(),
    // )));

    // commands.spawn(MaterialMeshBundle {
    //     mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 2., 2.) }.mesh())),
    //     material: materials.add(Color::srgb(0., 0., 1.)),
    //     transform: Transform::from_translation(Vec3::new(15., 1., 30.)),
    //     ..default()}
    // ).insert(SuppliesProductionComponent{
    //     supplies_storage_capacity: 9999999,
    //     available_supplies: 999999,
    //     supplies_production: (9999, ProductionData{
    //         time_to_produce: 5,
    //         resource_cost: 5,
    //         human_resource_cost: 5,
    //     }),
    //     elapsed_time: 0,
    //     nearest_road_point: (Vec3::new(25., 0., 50.), first_road_entity),
    // });

    let city_size = 150.;
    let city_buffer_zone = 500.;
    let city_road_connection_distance = 800.;

    let village_size = 100.;
    let village_buffer_zone = 300.;
    let village_road_connection_distance = 600.;

    for _i in 0..CITIES_COUNT {
        settlements.0.push((
            SettlementObject{
                team: player_data.team,
                settlement_size: city_size,
                buffer_zone_size: city_buffer_zone,
                max_road_connection_distance: city_road_connection_distance,
                connected_roads: Vec::new(),
                connected_settlements: Vec::new(),
                active_apartments: Vec::new(),
                human_resource_storage_capacity: 60,
                available_human_resources: 60,
                human_resource_production_rate: 6,
                human_resource_production_speed: 30000,
                production_local_point: Vec3::new(30., 0., 0.),
                elapsed_time: 0,
                time_to_capture: 20000,
                elapsed_capture_time: 0,
            },
            MaterialMeshBundle{
                mesh: meshes.add(Mesh::from(Cylinder{ radius: city_size, half_height: 5. }.mesh())),
                material: materials.add(Color::srgba(0., 1., 1., 0.25)),
                transform: Transform::from_translation(Vec3::ZERO),
                ..default()
            },
        ));
    }

    for _i in 0..VILLAGES_COUNT {
        settlements.0.push((
            SettlementObject{
                team: player_data.team,
                settlement_size: village_size,
                buffer_zone_size: village_buffer_zone,
                max_road_connection_distance: village_road_connection_distance,
                connected_roads: Vec::new(),
                connected_settlements: Vec::new(),
                active_apartments: Vec::new(),
                human_resource_storage_capacity: 30,
                available_human_resources: 30,
                human_resource_production_rate: 3,
                human_resource_production_speed: 30000,
                production_local_point: Vec3::new(30., 0., 0.),
                elapsed_time: 0,
                time_to_capture: 10000,
                elapsed_capture_time: 0,
            },
            MaterialMeshBundle{
                mesh: meshes.add(Mesh::from(Cylinder{ radius: village_size, half_height: 5. }.mesh())),
                material: materials.add(Color::srgba(0., 1., 1., 0.25)),
                transform: Transform::from_translation(Vec3::ZERO),
                ..default()
            },
        ));
    }

    // let artillery = MaterialMeshBundle {
    //     mesh: meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 1., 1.) }.mesh())),
    //     material: materials.add(Color::srgb(0., 0., 1.)),
    //     transform: Transform::from_translation(Vec3::new(75., 0.25, 25.)),
    //     ..default()
    // };

    // commands.spawn(artillery)
    // .insert(UnitComponent{
    //     path: Vec::new(),
    //     speed: 15.,
    // })
    // .insert(ArtilleryUnit{
    //     peak_trajectory_height: 100.,
    //     trajectory_points: 32,
    //     projectile_waypoints_check_factor: 5.,
    //     shell_speed: 200.,
    //     max_range: 600.,
    //     accuracy: 10.,
    //     reload_time: 3000,
    //     elapsed_reload_time: 0,
    // }).insert(SelectableUnit);

    // x = -5.;
    // z = 5.;

    // for _z in 0..1 {
    //     z += 5.;
    //     x = -5.;
    //     for _i in 0..100{
    //         x += 2.;
    //         let agent = MaterialMeshBundle {
    //             mesh: meshes.add(Mesh::from(Cylinder{ radius: 0.5, half_height: 1. }.mesh())),
    //             material: materials.add(Color::srgb(0., 0., 1.)),
    //             transform: Transform::from_translation(Vec3::new(x, 0.5, z)),
    //             ..default()
    //         };
    
    //         let agent_entity = commands.spawn(agent)
    //         .insert(Collider::cuboid(0.5, 1., 0.5))
    //         .insert(components::unit::UnitComponent{
    //             path: Vec::new(),
    //             speed: 10.,
    //         })
    //         .insert(components::unit::CombatComponent{
    //             team: 1,
    //             health: 100,
    //             damage: 30,
    //             accuracy: 0.5,
    //             enemies: Vec::new(),
    //             detection_range: 30.,
    //             tile_key: (0, 0),
    //             is_static: false,
    //         })
    //         .insert(components::unit::SelectableUnit)
    //         .id();
    
            
    //         tile_map.tiles.entry(((x / components::unit::TILE_SIZE) as i32, (z / components::unit::TILE_SIZE) as i32))
    //         .or_insert_with(HashMap::new).insert(agent_entity, (Vec3::new(x, 0., z), 1));
    //     }
    // }

    // x = -5.;
    // z = -70.;
    // for _z in 0..1 {
    //     z -= 5.;
    //     x = -5.;
    //     for _i in 0..100{
    //         x += 2.;
    //         let agent = MaterialMeshBundle {
    //             mesh: meshes.add(Mesh::from(Cylinder{ radius: 0.5, half_height: 1. }.mesh())),
    //             material: materials.add(Color::srgb(1., 0., 0.)),
    //             transform: Transform::from_translation(Vec3::new(x, 0.5, z)),
    //             ..default()
    //         };
    
    //         let agent_entity = commands.spawn(agent)
    //         .insert(Collider::cuboid(0.5, 1., 0.5))
    //         .insert(components::unit::UnitComponent{
    //             path: Vec::new(),
    //             speed: 10.,
    //         })
    //         .insert(components::unit::CombatComponent{
    //             team: 2,
    //             health: 100,
    //             damage: 30,
    //             accuracy: 0.5,
    //             enemies: Vec::new(),
    //             detection_range: 30.,
    //             tile_key: (0, 0),
    //             is_static: false,
    //             unit_data: (UnitTypes::None, (-1, -1, -1, -1, -1, -1, -1), "".to_string()),
    //         })
    //         .insert(components::unit::SelectableUnit)
    //         .id();
    
    //         tile_map.tiles.entry(((x / components::unit::TILE_SIZE) as i32, (z / components::unit::TILE_SIZE) as i32))
    //         .or_insert_with(HashMap::new).insert(agent_entity, (Vec3::new(x, 0., z), 2));
    //     }
    // }

    event_writer.send(SetupDoneEvent);
}

fn save_nav_mesh (
    keys: Res<ButtonInput<KeyCode>>,
    nav_mesh: Res<NavMesh>,
){
    if keys.just_pressed(KeyCode::KeyO) {
        let nav_mesh_got = nav_mesh.get();
        let nav_mesh_tiles = nav_mesh_got.read().unwrap();
        let _ = serialize_nav_mesh_tiles(nav_mesh_tiles.get_tiles(), "assets/OxidizedNavMeshExample.bin");
    }
}

fn load_naw_mesh (
    nav_mesh: Res<NavMesh>,
    mut commands: Commands,
){
    if std::path::Path::new("assets/OxidizedNavMeshExample.bin").exists() {
        let nav_mesh_got = nav_mesh.get();
        let mut nav_mesh_tiles = nav_mesh_got.write().unwrap();
        nav_mesh_tiles.set_tiles(deserialize_nav_mesh_tiles("assets/OxidizedNavMeshExample.bin"), commands);
    }
}

fn show_nav_mesh (
    keys: Res<ButtonInput<KeyCode>>,
    mut show_navmesh: ResMut<DrawNavMesh>,
){
    if keys.just_pressed(KeyCode::KeyN) {
        show_navmesh.0 = !show_navmesh.0;
    }
}

#[derive(Event)]
pub struct SetupDoneEvent;

fn delayed_load_naw_mesh (
    nav_mesh: Res<NavMesh>,
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
    mut is_tiles_set: Local<bool>,
){
    if *elapsed_time < 500 {
        *elapsed_time += time.delta().as_millis();
    } else if !*is_tiles_set {
        *is_tiles_set = true;
        if std::path::Path::new("assets/OxidizedNavMeshExample.bin").exists() {
            let nav_mesh_got = nav_mesh.get();
            let mut nav_mesh_tiles = nav_mesh_got.write().unwrap();
            nav_mesh_tiles.set_tiles(deserialize_nav_mesh_tiles("assets/OxidizedNavMeshExample.bin"), commands);
        }
    }
}

#[derive(Resource)]
pub struct GameStage(pub GameStages);

pub enum GameStages {
    SettlementsSetup,
    ArmySetup,
    BuildingsSetup,
    GameStarted,
}

#[derive(Resource)]
pub struct PlayerData{
    team: i32,
    is_all_settlements_placed: bool,
    is_ready_to_start: bool,
}

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup,(
            components::asset_manager::load_assets,
        ));
        app.add_systems(OnEnter(GameState::MainMenu),
            components::asset_manager::entity_cleaning_system,
        );
        app.add_systems(Update, (
            components::ui_manager::main_menu_ui_system,
        ).run_if(in_state(GameState::MainMenu)));
    }
}

pub struct GameEndPlugin;

impl Plugin for GameEndPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            components::ui_manager::game_end_ui_system,
        ).run_if(in_state(GameState::GameEnd)));
    }
}

pub struct SingleplayerPlugin;

impl Plugin for SingleplayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Singleplayer),(
                setup,
                setup_ingame_ui,
                settlements_stage_ui_activation,
        ));
        app.add_systems(Update, (
            components::camera::camera_system,
            components::unit::artillery_designation_system,
            components::camera::handle_mouse_buttons,
            components::camera::update_selection_box,
            components::unit::process_manual_pathfinding,
            components::unit::poll_pathfinding_tasks_system,
            components::unit::process_agents_movement,
            components::unit::check_tiles,
            components::unit::find_targets,
            components::unit::process_combat,
            components::ui_manager::handle_button_clicks,
            components::ui_manager::land_army_settings_system,
            components::ui_manager::open_company_type_dropdown_list,
            components::ui_manager::choose_company_type,
            components::ui_manager::setup_company,
            components::ui_manager::open_specializations_dropdown_list,
            components::ui_manager::choose_squad_specialization,
            components::building::unit_production_system,
            components::ui_manager::toggle_production,
        ).run_if(in_state(GameState::Singleplayer)));
        app.add_systems(Update, (
            components::building::production_manager,
            components::building::unit_replenishment_system,
            components::unit::platoon_leaders_monitoring_system,
            components::ui_manager::platoon_nodes_positioning_system,
            components::unit::squad_selection_system,
            components::unit::cover_disturb_system,
            components::unit::cover_assignation_system,
            components::unit::unit_covering_system,
            components::building::unit_uncovering_system,
            components::ui_manager::toggle_buildings_list_system,
            components::ui_manager::building_placement_activation_system,
            components::ui_manager::building_placement_handling_system,
            components::unit::engineer_to_blueprint_assignation_system,
            components::unit::process_busy_engineers,
            components::logistics::assign_supply_tasks,
            components::logistics::logistic_convoys_processing_system,
            components::building::settlements_placement_system,
            components::building::apartments_generation_system,
            components::building::roads_generation_system,
            components::building::resource_zones_generation_system,
        ).run_if(in_state(GameState::Singleplayer)));
        app.add_systems(Update, (
            save_nav_mesh,
            show_nav_mesh,
            components::building::temporary_objects_deletion_system,
            components::ui_manager::toggle_artillety_management_node,
            components::unit::toggle_artillery_designation,
            components::unit::artillery_order_delayed,
            components::unit::artillery_firing_system,
            components::unit::artillery_shells_movement_system,
            components::ui_manager::building_stage_ui_activation,
            components::ui_manager::army_setup_stage_ui_activation,
            components::unit::game_starting_system,
            components::unit::homing_projectiles_moving_system,
            components::unit::explosion_processing_system,
            components::logistics::material_producers_processing_system,
            components::logistics::human_resource_producers_processing_system,
            components::asset_manager::initialize_level_gltf_objects,
            components::asset_manager::ground_line_highlighter,
            components::asset_manager::blueprint_placement_color_definer,
            components::asset_manager::lod_system,
            components::asset_manager::new_lods_initializing_system,
        ).run_if(in_state(GameState::Singleplayer)));
        app.add_systems(Update, (
            delayed_load_naw_mesh,
            components::building::settlements_capturing_system,
            components::ui_manager::tactical_symbols_dropdown_menu_system,
            components::ui_manager::tactical_symbols_level_choose_system,
            components::unit::platoon_selection_system,
            components::unit::company_selection_system,
            components::unit::battalion_selection_system,
            components::unit::regiment_selection_system,
            components::unit::brigade_selection_system,
            components::unit::artillery_unit_selection_system,
            components::unit::update_fog_of_war,
            components::unit::visual_projectiles_processing_system,
            components::asset_manager::trail_processing_system,
            components::unit::supplies_consumption_system,
            components::logistics::supplies_production_system,
            components::building::resources_amount_displays_processing_system,
            components::ui_manager::overall_resources_amount_updating_system,
            components::building::construction_progress_displays_processing_system,
            components::building::buildings_deletion_activation_system,
            components::building::blueprints_deletion_system,
        ).run_if(in_state(GameState::Singleplayer)));
        app.add_systems(Update, (
            components::building::buildings_deletion_system,
            components::building::buildings_deletion_cancelation_system,
            components::ui_manager::switchable_buildings_ui_manager,
            components::building::buildings_state_switcher,
            components::ui_manager::rebuild_settlement_ui_manager,
            components::building::rebuild_settlement_apartments_system,
            components::building::apartments_rebuilding_system,
            components::building::capturing_displays_processing_system,
            components::ui_manager::hint_management_system,
            components::asset_manager::testing_system,
            components::asset_manager::apply_team_material_to_scenes,
            components::asset_manager::running_animation_manager,
            components::asset_manager::explosion_effects_handler,
            components::unit::transport_assignation_system,
            components::unit::transport_disturb_system,
            components::unit::transport_embark_system,
            components::unit::transport_disembark_system,
            components::ui_manager::disembark_button_system,
            components::unit::remains_processing_system,
        ).run_if(in_state(GameState::Singleplayer)));
        app.add_systems(Update, (
            components::logistics::logistic_units_unstuck_system,
            components::unit::pathfinding_tasks_starter,
            components::ui_manager::regiment_swipe_system,
            components::ui_manager::esc_menu_ui_system,
            components::ui_manager::ui_nodes_unlocker,//keep last
        ).run_if(in_state(GameState::Singleplayer)));
    }
}

pub struct LobbyServerPlugin;

impl Plugin for LobbyServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::LobbyAsServer),(
            components::network::initialize_server_lobby,
            components::network::start_listening_clients,
        ));
        app.add_systems(Update, (
            components::network::client_messages_handler,
            components::ui_manager::show_lobby_as_server,
        ).run_if(in_state(GameState::LobbyAsServer)));
    }
}

pub struct LobbyClientPlugin;

impl Plugin for LobbyClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::LobbyAsClient),(
            components::network::start_connection_to_server,
        ));
        app.add_systems(Update, (
            components::network::server_messages_handler,
            components::ui_manager::show_lobby_as_client,
            components::network::client_game_initialization_system,
        ).run_if(in_state(GameState::LobbyAsClient)));
    }
}

pub struct GameServerPlugin;

impl Plugin for GameServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::MultiplayerAsHost),(
            setup,
            setup_ingame_ui,
            settlements_stage_ui_activation,
        ));
        app.add_systems(Update, (
            components::network::client_messages_handler,
            components::network::mp_game_starter,
            components::network::mp_settlements_placement_completion,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            components::camera::camera_system,
            components::unit::artillery_designation_system,
            components::camera::handle_mouse_buttons,
            components::camera::update_selection_box,
            components::unit::process_manual_pathfinding,
            components::unit::poll_pathfinding_tasks_system,
            components::unit::process_agents_movement,
            components::unit::check_tiles,
            components::unit::find_targets,
            components::unit::process_combat,
            components::ui_manager::handle_button_clicks,
            components::ui_manager::land_army_settings_system,
            components::ui_manager::open_company_type_dropdown_list,
            components::ui_manager::choose_company_type,
            components::ui_manager::setup_company,
            components::ui_manager::open_specializations_dropdown_list,
            components::ui_manager::choose_squad_specialization,
            components::building::unit_production_system,
            components::ui_manager::toggle_production,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            components::building::production_manager,
            components::building::unit_replenishment_system,
            components::unit::platoon_leaders_monitoring_system,
            components::ui_manager::platoon_nodes_positioning_system,
            components::unit::squad_selection_system,
            components::unit::cover_disturb_system,
            components::unit::cover_assignation_system,
            components::unit::unit_covering_system,
            components::building::unit_uncovering_system,
            components::ui_manager::toggle_buildings_list_system,
            components::ui_manager::building_placement_activation_system,
            components::ui_manager::building_placement_handling_system,
            components::unit::engineer_to_blueprint_assignation_system,
            components::unit::process_busy_engineers,
            components::logistics::assign_supply_tasks,
            components::logistics::logistic_convoys_processing_system,
            components::building::settlements_placement_system,
            components::building::apartments_generation_system,
            components::building::roads_generation_system,
            components::building::resource_zones_generation_system,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            // save_nav_mesh,
            // show_nav_mesh,
            components::building::temporary_objects_deletion_system,
            components::ui_manager::toggle_artillety_management_node,
            components::unit::toggle_artillery_designation,
            components::unit::artillery_order_delayed,
            components::unit::artillery_firing_system,
            components::unit::artillery_shells_movement_system,
            components::ui_manager::building_stage_ui_activation,
            components::ui_manager::army_setup_stage_ui_activation,
            components::unit::game_starting_system,
            components::unit::homing_projectiles_moving_system,
            components::unit::explosion_processing_system,
            components::logistics::material_producers_processing_system,
            components::logistics::human_resource_producers_processing_system,
            components::asset_manager::initialize_level_gltf_objects,
            components::asset_manager::ground_line_highlighter,
            components::asset_manager::blueprint_placement_color_definer,
            components::asset_manager::lod_system,
            components::asset_manager::new_lods_initializing_system,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            delayed_load_naw_mesh,
            components::building::settlements_capturing_system,
            components::ui_manager::tactical_symbols_dropdown_menu_system,
            components::ui_manager::tactical_symbols_level_choose_system,
            components::unit::platoon_selection_system,
            components::unit::company_selection_system,
            components::unit::battalion_selection_system,
            components::unit::regiment_selection_system,
            components::unit::brigade_selection_system,
            components::unit::update_fog_of_war,
            components::unit::visual_projectiles_processing_system,
            components::asset_manager::trail_processing_system,
            components::unit::supplies_consumption_system,
            components::logistics::supplies_production_system,
            components::building::resources_amount_displays_processing_system,
            components::ui_manager::overall_resources_amount_updating_system,
            components::building::construction_progress_displays_processing_system,
            components::building::buildings_deletion_activation_system,
            components::building::blueprints_deletion_system,
            components::building::buildings_deletion_system,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            components::building::buildings_deletion_cancelation_system,
            components::ui_manager::switchable_buildings_ui_manager,
            components::building::buildings_state_switcher,
            components::ui_manager::rebuild_settlement_ui_manager,
            components::building::rebuild_settlement_apartments_system,
            components::building::apartments_rebuilding_system,
            components::building::capturing_displays_processing_system,
            components::ui_manager::hint_management_system,
            components::asset_manager::testing_system,
            components::asset_manager::apply_team_material_to_scenes,
            components::asset_manager::running_animation_manager,
            components::asset_manager::explosion_effects_handler,
            components::unit::transport_assignation_system,
            components::unit::transport_disturb_system,
            components::unit::transport_embark_system,
            components::unit::transport_disembark_system,
            components::ui_manager::disembark_button_system,
            components::unit::remains_processing_system,
            components::logistics::logistic_units_unstuck_system,
        ).run_if(in_state(GameState::MultiplayerAsHost)));
        app.add_systems(Update, (
            components::unit::artillery_unit_selection_system,
            components::ui_manager::regiment_swipe_system,
            components::ui_manager::esc_menu_ui_system,
            components::ui_manager::ui_nodes_unlocker,//keep last
        ).run_if(in_state(GameState::MultiplayerAsHost)));
    }
}

pub struct GameClientPlugin;

impl Plugin for GameClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::MultiplayerAsClient),(
            setup,
            setup_ingame_ui,
            settlements_stage_ui_activation,
        ));
        app.add_systems(Update, (
            components::network::server_messages_handler,
            components::network::client_settlements_placement_completion,
            components::network::client_game_starting_system,
            components::network::client_entity_movement_system,
        ).run_if(in_state(GameState::MultiplayerAsClient)));
        app.add_systems(Update, (
            components::camera::camera_system,
            components::unit::artillery_designation_system,
            components::camera::handle_mouse_buttons,
            components::camera::update_selection_box,
            components::unit::process_manual_pathfinding,
            components::unit::poll_pathfinding_tasks_system,
            components::unit::process_agents_movement,
            components::unit::check_tiles,
            components::unit::find_targets,
            components::unit::process_combat,
            components::ui_manager::handle_button_clicks,
            components::ui_manager::land_army_settings_system,
            components::ui_manager::open_company_type_dropdown_list,
            components::ui_manager::choose_company_type,
            components::ui_manager::setup_company,
            components::ui_manager::open_specializations_dropdown_list,
            components::ui_manager::choose_squad_specialization,
            components::ui_manager::toggle_production,
        ).run_if(in_state(GameState::MultiplayerAsClient)));
        app.add_systems(Update, (
            components::unit::platoon_leaders_monitoring_system,
            components::ui_manager::platoon_nodes_positioning_system,
            components::unit::squad_selection_system,
            components::unit::cover_disturb_system,
            components::unit::cover_assignation_system,
            components::ui_manager::toggle_buildings_list_system,
            components::ui_manager::building_placement_activation_system,
            components::ui_manager::building_placement_handling_system,
            components::building::settlements_placement_system,
        ).run_if(in_state(GameState::MultiplayerAsClient)));
        app.add_systems(Update, (
            // save_nav_mesh,
            // show_nav_mesh,
            components::building::temporary_objects_deletion_system,
            components::ui_manager::toggle_artillety_management_node,
            components::unit::toggle_artillery_designation,
            components::unit::artillery_order_delayed,
            components::unit::explosion_processing_system,
            components::ui_manager::building_stage_ui_activation,
            components::ui_manager::army_setup_stage_ui_activation,
            components::asset_manager::initialize_level_gltf_objects,
            components::asset_manager::ground_line_highlighter,
            components::asset_manager::blueprint_placement_color_definer,
            components::asset_manager::lod_system,
            components::asset_manager::new_lods_initializing_system,
            delayed_load_naw_mesh,
        ).run_if(in_state(GameState::MultiplayerAsClient)));
        app.add_systems(Update, (
            components::ui_manager::tactical_symbols_dropdown_menu_system,
            components::ui_manager::tactical_symbols_level_choose_system,
            components::unit::platoon_selection_system,
            components::unit::company_selection_system,
            components::unit::battalion_selection_system,
            components::unit::regiment_selection_system,
            components::unit::brigade_selection_system,
            components::unit::update_fog_of_war,
            components::unit::visual_projectiles_processing_system,
            components::asset_manager::trail_processing_system,
            components::unit::supplies_consumption_system,
            components::building::resources_amount_displays_processing_system,
            components::building::construction_progress_displays_processing_system,
            components::building::buildings_deletion_activation_system,
            components::building::blueprints_deletion_system,
            components::building::buildings_deletion_system,
            components::building::buildings_deletion_cancelation_system,
            components::ui_manager::switchable_buildings_ui_manager,
            components::building::buildings_state_switcher,
            components::ui_manager::rebuild_settlement_ui_manager,
        ).run_if(in_state(GameState::MultiplayerAsClient)));
        app.add_systems(Update, (
            components::building::rebuild_settlement_apartments_system,
            components::building::capturing_displays_processing_system,
            components::unit::transport_assignation_system,
            components::unit::transport_disturb_system,
            components::unit::transport_disembark_system,
            components::ui_manager::disembark_button_system,
            components::unit::remains_processing_system,
            components::ui_manager::hint_management_system,
            components::asset_manager::testing_system,
            components::asset_manager::apply_team_material_to_scenes,
            components::asset_manager::running_animation_manager,
            components::asset_manager::explosion_effects_handler,
            components::unit::remains_processing_system,
            components::unit::artillery_unit_selection_system,
            components::ui_manager::regiment_swipe_system,
            components::ui_manager::esc_menu_ui_system,
            components::ui_manager::ui_nodes_unlocker,//keep last
        ).run_if(in_state(GameState::MultiplayerAsClient)));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
pub enum GameState{
    #[default]
    MainMenu,
    Singleplayer,
    LobbyAsServer,
    LobbyAsClient,
    MultiplayerAsHost,
    MultiplayerAsClient,
    GameEnd,
}