use std::{f32::consts::TAU, primitive};

use bevy::{gltf::GltfMesh, pbr::{ExtendedMaterial, MaterialExtension, NotShadowCaster}, prelude::*, render::{mesh::{Indices, MeshVertexBufferLayout}, render_asset::RenderAssetUsages, render_resource::{AsBindGroup, DynamicUniformBuffer, PipelineDescriptor, RenderPipelineDescriptor, Sampler, ShaderRef, ShaderType, SpecializedMeshPipelineError}, texture::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor}}, transform::commands, utils::{hashbrown::HashMap, HashSet}};
use bevy_rapier3d::{na::TGeneral, prelude::{Collider, CollisionGroups, ComputedColliderShape, Group}};
use oxidized_navigation_serializable::NavMeshAffector;

use crate::{components::{building::{BuildingBlueprint, UnactivatedBlueprints}, camera::CameraComponent, ui_manager::DisplayedModelHolder, unit::{AttackTypes, CombatComponent, CompanyTypes, FogOfWarTexture, UnitTypes}}, WORLD_SIZE};

#[derive(Resource)]
pub struct LevelAssets {
    pub landscape: Handle<Gltf>,
    pub grass_texture: Handle<Image>,
    pub stone_texture: Handle<Image>,
    pub snow_texture: Handle<Image>,

    pub trees_2d: Handle<Scene>,
    pub trees_3d: Handle<Scene>,
}

#[derive(Resource)]
pub struct BuildingsAssets {
    pub barracks: (Handle<Mesh>, Handle<StandardMaterial>),
    pub vehicle_factory: (Handle<Mesh>, Handle<StandardMaterial>),
    pub logistic_hub: (Handle<Mesh>, Handle<StandardMaterial>),
    pub resource_extractor: (Handle<Mesh>, Handle<StandardMaterial>),
    pub pillbox: (Handle<Mesh>, Handle<StandardMaterial>),

    pub town_hall: (Handle<Mesh>, Handle<StandardMaterial>),
    pub apartment: (Handle<Mesh>, Handle<StandardMaterial>),
}

#[derive(Resource)]
pub struct UnitsAssets {
    pub regular_soldier: (Handle<Mesh>, Handle<StandardMaterial>),
    pub assault_soldier: (Handle<Mesh>, Handle<StandardMaterial>),
    pub atgm_soldier: (Handle<Mesh>, Handle<StandardMaterial>),
    pub rpg_soldier: (Handle<Mesh>, Handle<StandardMaterial>),
    pub sniper_soldier: (Handle<Mesh>, Handle<StandardMaterial>),
    pub tank: (Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>),
    pub ifv: (Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>),
    pub artillery: (Handle<Mesh>, Handle<StandardMaterial>),
    pub truck: (Handle<Mesh>, Handle<StandardMaterial>),
    pub engineer: (Handle<Mesh>, Handle<StandardMaterial>),
}

#[derive(Resource)]
pub struct OtherAssets {
    pub regular_infantry_squad_symbol: Handle<Image>,
    pub shock_infantry_squad_symbol: Handle<Image>,
    pub armored_squad_symbol: Handle<Image>,
    pub artillery_unit_symbol: Handle<Image>,
    pub engineer_unit_symbol: Handle<Image>,

    pub regular_infantry_platoon_symbol: Handle<Image>,
    pub shock_infantry_platoon_symbol: Handle<Image>,
    pub armored_platoon_symbol: Handle<Image>,

    pub regular_infantry_company_symbol: Handle<Image>,
    pub shock_infantry_company_symbol: Handle<Image>,
    pub armored_company_symbol: Handle<Image>,

    pub battalion_symbol: Handle<Image>,

    pub regiment_symbol: Handle<Image>,

    pub brigade_symbol: Handle<Image>,

    pub materials_icon: Handle<Image>,

    pub human_resource_icon: Handle<Image>,
}

#[derive(Resource)]

pub struct AttackVisualisationAssets {
    pub bullet_low: (PbrBundle, Handle<AudioSource>),
    pub bullet_high: (PbrBundle, Handle<AudioSource>),

    pub shell: (Handle<Mesh>, Handle<StandardMaterial>),

    pub missile_launch_sound: Handle<AudioSource>,
    pub tank_shot_sound: Handle<AudioSource>,
    pub explosion_small_sound: Handle<AudioSource>,
    pub explosion_big_sound: Handle<AudioSource>,
}

#[derive(Clone, Copy, Default, ShaderType, Debug, Reflect)]
#[repr(C)]
pub struct LineData{
    pub line_start: Vec2,
    pub line_end: Vec2,
    pub line_width: f32,
    pub highlight_color: Vec4,
}

#[derive(Clone, Copy, Default, ShaderType, Debug, Reflect)]
#[repr(C)]
pub struct CircleData{
    pub circle_center: Vec2,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub highlight_color: Vec4,
}

#[derive(Component)]
pub struct LineHolder(pub Vec<LineData>);

#[derive(Component)]
pub struct CircleHolder(pub Vec<CircleData>);

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainMaterialExtension {
    #[storage(100, read_only)]
    pub lines: Vec<LineData>,
    #[uniform(101)]
    pub line_count: u32,

    #[storage(102, read_only)]
    pub circles: Vec<CircleData>,
    #[uniform(103)]
    pub circle_count: u32,

    #[texture(104)]
    #[sampler(105)]
    pub grass_texture: Handle<Image>,

    #[texture(106)]
    #[sampler(107)]
    pub stone_texture: Handle<Image>,

    #[texture(108)]
    #[sampler(109)]
    pub snow_texture: Handle<Image>,

    #[uniform(110)]
    pub height_factors: Vec2,

    #[uniform(111)]
    pub repeat_factor: f32,

    #[uniform(112)]
    pub world_size: f32,

    #[texture(113)]
    #[sampler(114)]
    pub fog_of_war_texture: Handle<Image>,
}

impl MaterialExtension for TerrainMaterialExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_material_shader.wgsl".into()
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TeamMaterialExtension {
    #[uniform(120)]
    pub team_color: Vec4,
}

impl MaterialExtension for TeamMaterialExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/team_shader.wgsl".into()
    }
}

pub fn generate_circle_segments(center: Vec2, radius: f32, segments: usize) -> Vec<(Vec2, Vec2)> {
    let mut points = Vec::with_capacity(segments);

    for i in 0..segments {
        let angle = TAU * (i as f32 / segments as f32);
        let x = center.x + radius * angle.cos();
        let y = center.y + radius * angle.sin();
        points.push(Vec2::new(x, y));
    }

    let mut segments_vec = Vec::with_capacity(segments);
    for i in 0..segments {
        let next_i = (i + 1) % segments;
        segments_vec.push((points[i], points[next_i]));
    }

    segments_vec
}

pub fn load_assets (
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
){
    //Level
    let gltf: Handle<Gltf> = asset_server.load("landscape/Terrain.glb");
    let grass: Handle<Image> = asset_server.load_with_settings(
        "textures/terrain/grass_texture.png",
        |s: &mut _| {
            *s = ImageLoaderSettings {
                sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..default()
                }),
                ..default()
            }
        },
    );
    let stone: Handle<Image> = asset_server.load_with_settings(
        "textures/terrain/stone_texture.png",
        |s: &mut _| {
            *s = ImageLoaderSettings {
                sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..default()
                }),
                ..default()
            }
        },
    );
    let snow: Handle<Image> = asset_server.load_with_settings(
        "textures/terrain/snow_texture.png",
        |s: &mut _| {
            *s = ImageLoaderSettings {
                sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..default()
                }),
                ..default()
            }
        },
    );

    let trees_2d: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("landscape/trees2d.glb"));
    let trees_3d: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("landscape/trees3d.glb"));

    commands.insert_resource(LevelAssets{
        landscape: gltf,
        grass_texture: grass,
        stone_texture: stone,
        snow_texture: snow,
        trees_2d: trees_2d,
        trees_3d: trees_3d,
    });
    //Level^

    //Buildings
    let barracks_mesh: Handle<Mesh> = asset_server.load("buildings/barracks.glb#Mesh0/Primitive0");
    let barracks_material: Handle<StandardMaterial> = asset_server.load("buildings/barracks.glb#Material0");

    let vehicle_factory_mesh: Handle<Mesh> = asset_server.load("buildings/VehicleFactory.glb#Mesh0/Primitive0");
    let vehicle_factory_material: Handle<StandardMaterial> = asset_server.load("buildings/VehicleFactory.glb#Material0");

    let logistic_hub_mesh: Handle<Mesh> = asset_server.load("buildings/LogisticHub.glb#Mesh0/Primitive0");
    let logistic_hub_material: Handle<StandardMaterial> = asset_server.load("buildings/LogisticHub.glb#Material0");

    let resource_extractor_mesh: Handle<Mesh> = asset_server.load("buildings/ResourceExtractor.glb#Mesh0/Primitive0");
    let resource_extractor_material: Handle<StandardMaterial> = asset_server.load("buildings/ResourceExtractor.glb#Material0");

    let pillbox_mesh: Handle<Mesh> = asset_server.load("buildings/pillbox.glb#Mesh0/Primitive0");
    let pillbox_material: Handle<StandardMaterial> = asset_server.load("buildings/pillbox.glb#Material0");

    let town_hall_mesh: Handle<Mesh> = asset_server.load("buildings/TownHall.glb#Mesh0/Primitive0");
    let town_hall_material: Handle<StandardMaterial> = asset_server.load("buildings/TownHall.glb#Material0");

    let apartment_mesh: Handle<Mesh> = asset_server.load("buildings/Apartment.glb#Mesh0/Primitive0");
    let apartment_material: Handle<StandardMaterial> = asset_server.load("buildings/Apartment.glb#Material0");

    commands.insert_resource(BuildingsAssets{
        barracks: (barracks_mesh, barracks_material),
        vehicle_factory: (vehicle_factory_mesh, vehicle_factory_material),
        logistic_hub: (logistic_hub_mesh, logistic_hub_material),
        resource_extractor: (resource_extractor_mesh, resource_extractor_material),
        pillbox: (pillbox_mesh, pillbox_material),

        town_hall: (town_hall_mesh, town_hall_material),
        apartment: (apartment_mesh, apartment_material),
    });
    //Buildings^

    //Units
    let regular_soldier_mesh: Handle<Mesh> = asset_server.load("units/soldier.glb#Mesh0/Primitive0");
    let regular_soldier_material: Handle<StandardMaterial> = asset_server.load("units/soldier.glb#Material0");

    let assault_soldier_mesh: Handle<Mesh> = asset_server.load("units/soldier_assault.glb#Mesh0/Primitive0");
    let assault_soldier_material: Handle<StandardMaterial> = asset_server.load("units/soldier_assault.glb#Material0");

    let atgm_soldier_mesh: Handle<Mesh> = asset_server.load("units/soldier_atgm.glb#Mesh0/Primitive0");
    let atgm_soldier_material: Handle<StandardMaterial> = asset_server.load("units/soldier_atgm.glb#Material0");

    let rpg_soldier_mesh: Handle<Mesh> = asset_server.load("units/soldier_rpg.glb#Mesh0/Primitive0");
    let rpg_soldier_material: Handle<StandardMaterial> = asset_server.load("units/soldier_rpg.glb#Material0");

    let sniper_soldier_mesh: Handle<Mesh> = asset_server.load("units/soldier_sniper.glb#Mesh0/Primitive0");
    let sniper_soldier_material: Handle<StandardMaterial> = asset_server.load("units/soldier_sniper.glb#Material0");

    let tank_mesh_hull: Handle<Mesh> = asset_server.load("units/tank.glb#Mesh0/Primitive0");
    let tank_mesh_turret: Handle<Mesh> = asset_server.load("units/tank.glb#Mesh1/Primitive0");
    let tank_material: Handle<StandardMaterial> = asset_server.load("units/tank.glb#Material0");

    let ifv_mesh_hull: Handle<Mesh> = asset_server.load("units/ifv.glb#Mesh0/Primitive0");
    let ifv_mesh_turret: Handle<Mesh> = asset_server.load("units/ifv.glb#Mesh1/Primitive0");
    let ifv_material: Handle<StandardMaterial> = asset_server.load("units/ifv.glb#Material0");

    let artillery_mesh: Handle<Mesh> = asset_server.load("units/artillery.glb#Mesh0/Primitive0");
    let artillery_material: Handle<StandardMaterial> = asset_server.load("units/artillery.glb#Material0");

    let truck_mesh: Handle<Mesh> = asset_server.load("units/truck.glb#Mesh0/Primitive0");
    let truck_material: Handle<StandardMaterial> = asset_server.load("units/truck.glb#Material0");

    let engineer_mesh: Handle<Mesh> = asset_server.load("units/engineer.glb#Mesh0/Primitive0");
    let engineer_material: Handle<StandardMaterial> = asset_server.load("units/engineer.glb#Material0");

    commands.insert_resource(UnitsAssets{
        regular_soldier: (regular_soldier_mesh, regular_soldier_material),
        assault_soldier: (assault_soldier_mesh, assault_soldier_material),
        atgm_soldier: (atgm_soldier_mesh, atgm_soldier_material),
        rpg_soldier: (rpg_soldier_mesh, rpg_soldier_material),
        sniper_soldier: (sniper_soldier_mesh, sniper_soldier_material),
        tank: (tank_mesh_hull, tank_mesh_turret, tank_material),
        ifv: (ifv_mesh_hull, ifv_mesh_turret, ifv_material),
        artillery: (artillery_mesh, artillery_material),
        truck: (truck_mesh, truck_material),
        engineer: (engineer_mesh, engineer_material),
    });
    //Units^

    //Other
    let regular_infantry_symbol: Handle<Image> = asset_server.load("icons/tactical/regular_infantry_squad.png");
    let shock_infantry_symbol: Handle<Image> = asset_server.load("icons/tactical/shock_infantry_squad.png");
    let armored_symbol: Handle<Image> = asset_server.load("icons/tactical/armored_squad.png");
    let artillery_symbol: Handle<Image> = asset_server.load("icons/tactical/artillery_unit.png");
    let engineer_symbol: Handle<Image> = asset_server.load("icons/tactical/engineer_unit.png");

    let regular_infantry_platoon_symbol: Handle<Image> = asset_server.load("icons/tactical/regular_infantry_platoon.png");
    let shock_infantry_platoon_symbol: Handle<Image> = asset_server.load("icons/tactical/shock_infantry_platoon.png");
    let armored_platoon_symbol: Handle<Image> = asset_server.load("icons/tactical/armored_platoon.png");

    let regular_infantry_company_symbol: Handle<Image> = asset_server.load("icons/tactical/regular_infantry_company.png");
    let shock_infantry_company_symbol: Handle<Image> = asset_server.load("icons/tactical/shock_infantry_company.png");
    let armored_company_symbol: Handle<Image> = asset_server.load("icons/tactical/armored_company.png");

    let battalion_symbol: Handle<Image> = asset_server.load("icons/tactical/battalion.png");

    let regiment_symbol: Handle<Image> = asset_server.load("icons/tactical/regiment.png");

    let brigade_symbol: Handle<Image> = asset_server.load("icons/tactical/brigade.png");

    let materials_icon: Handle<Image> = asset_server.load("icons/resources/materials.png");

    let human_resource_icon: Handle<Image> = asset_server.load("icons/resources/human.png");

    commands.insert_resource(OtherAssets{
        regular_infantry_squad_symbol: regular_infantry_symbol,
        shock_infantry_squad_symbol: shock_infantry_symbol,
        armored_squad_symbol: armored_symbol,
        artillery_unit_symbol: artillery_symbol,
        engineer_unit_symbol: engineer_symbol,

        regular_infantry_platoon_symbol: regular_infantry_platoon_symbol,
        shock_infantry_platoon_symbol: shock_infantry_platoon_symbol,
        armored_platoon_symbol: armored_platoon_symbol,

        regular_infantry_company_symbol: regular_infantry_company_symbol,
        shock_infantry_company_symbol: shock_infantry_company_symbol,
        armored_company_symbol: armored_company_symbol,

        battalion_symbol: battalion_symbol,

        regiment_symbol: regiment_symbol,

        brigade_symbol: brigade_symbol,

        materials_icon: materials_icon,
        
        human_resource_icon: human_resource_icon,
    });
    //Other^

    //Attack visualisation
    let bullet_low: Handle<Image> = asset_server.load("textures/bullets/bullet_low.png");
    let bullet_low_sound: Handle<AudioSource> = asset_server.load("audio/gunshots/low_cal_burst.ogg");

    let bullet_low_material = materials.add(StandardMaterial{
        base_color_texture: Some(bullet_low),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default() 
    });

    let bullet_high: Handle<Image> = asset_server.load("textures/bullets/bullet_high.png");
    let bullet_high_sound: Handle<AudioSource> = asset_server.load("audio/autocannon/autocannon.ogg");

    let bullet_high_material = materials.add(StandardMaterial{
        base_color_texture: Some(bullet_high),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default() 
    });

    let shell_mesh: Handle<Mesh> = asset_server.load("other/shell.glb#Mesh0/Primitive0");
    let shell_material: Handle<StandardMaterial> = asset_server.load("other/shell.glb#Material0");

    let missile_launch_sound: Handle<AudioSource> = asset_server.load("audio/missile_launch/missile_launch.ogg");

    let tank_shot_sound: Handle<AudioSource> = asset_server.load("audio/cannon/cannon.ogg");

    let explosion_small_sound: Handle<AudioSource> = asset_server.load("audio/explosions/explosion_small.ogg");

    let explosion_big_sound: Handle<AudioSource> = asset_server.load("audio/explosions/explosion_big.ogg");

    commands.insert_resource(AttackVisualisationAssets{
        bullet_low: (
            PbrBundle{
                mesh: meshes.add(Mesh::from(Plane3d::default().mesh().size(0.1, 1.))),
                material: bullet_low_material,
                ..default()
            },
            bullet_low_sound,
        ),
        bullet_high: (
            PbrBundle{
                mesh: meshes.add(Mesh::from(Plane3d::default().mesh().size(0.2, 2.))),
                material: bullet_high_material,
                ..default()
            },
            bullet_high_sound,
        ),
        shell: (shell_mesh, shell_material),
        missile_launch_sound: missile_launch_sound,
        tank_shot_sound: tank_shot_sound,
        explosion_small_sound: explosion_small_sound,
        explosion_big_sound: explosion_big_sound,
    });
    //Attack visualisation^
}

#[derive(Component)]
pub struct Terrain;

pub fn initialize_level_gltf_objects (
    mut commands: Commands,
    level_assets: Res<LevelAssets>,
    gltf_assets: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TerrainMaterialExtension>>>,
    fog_of_war_texture: Res<FogOfWarTexture>,
    asset_server: Res<AssetServer>,
    mut loaded: Local<bool>,
){
    if *loaded {
        return;
    }

    let Some(gltf) = gltf_assets.get(&level_assets.landscape) else {
        return;
    };
    *loaded = true;

    if let Some(gltf_mesh) = gltf_meshes.get(&gltf.meshes[0].clone()) {
        let mesh_handle = gltf_mesh.primitives[0].mesh.clone();
        if let Some(mesh) = meshes.get(&mesh_handle) {
            if let Some(collider) = Collider::from_bevy_mesh(mesh, &ComputedColliderShape::TriMesh) {
                commands.spawn(MaterialMeshBundle{
                    mesh: mesh_handle,
                    material: extended_materials.add(ExtendedMaterial {
                        base: StandardMaterial {
                            ..default()
                        },
                        extension: TerrainMaterialExtension {
                            lines: vec![],
                            line_count: 0,
                            circles: vec![],
                            circle_count: 0,
                            grass_texture: level_assets.grass_texture.clone(),
                            stone_texture: level_assets.stone_texture.clone(),
                            snow_texture: level_assets.snow_texture.clone(),
                            height_factors: Vec2::new(5., 55.),
                            repeat_factor: 30.,
                            world_size: WORLD_SIZE,
                            fog_of_war_texture: fog_of_war_texture.handle.clone(),
                        },
                    }),
                    transform: Transform::from_translation(Vec3::new(0., 0., 0.)),
                    ..default()
                })
                .insert(collider)
                .insert(NavMeshAffector)
                .insert(Terrain);

                commands.spawn(SceneBundle{
                    scene: level_assets.trees_2d.clone(),
                    ..default()
                })
                .insert(Name::new("trees_2d"));

                // commands.spawn(MaterialMeshBundle{
                //     mesh: mesh_handle,
                //     material: terrain_material.add(TerrainMaterial{
                //         grass_texture: level_assets.grass_texture.clone(),
                //         stone_texture: level_assets.stone_texture.clone(),
                //         snow_texture: level_assets.snow_texture.clone(),
                //         height_factors: Vec2::new(10., 30.),
                //         repeat_factor: 100.,
                //         alpha_mode: AlphaMode::Blend,
                //     }),
                //     transform: Transform::from_translation(Vec3::new(0., 0., 0.)),
                //     ..default()
                // });

                // commands.spawn(MaterialMeshBundle{
                //     mesh: meshes.add(Mesh::from(Plane3d::default().mesh().size(WORLD_SIZE, WORLD_SIZE))),
                //     material: extended_materials.add(ExtendedMaterial {
                //         base: StandardMaterial {
                //             base_color: Color::srgba(1., 1., 1., 1.),
                //             alpha_mode: AlphaMode::Blend,
                //             ..default()
                //         },
                //         extension: TerrainMaterialExtension {
                //             lines: vec![],
                //             line_count: 0,
                //             circles: vec![],
                //             circle_count: 0,
                //             grass_texture: level_assets.grass_texture.clone(),
                //             stone_texture: level_assets.stone_texture.clone(),
                //             snow_texture: level_assets.snow_texture.clone(),
                //             height_factors: Vec2::new(5., 55.),
                //             repeat_factor: 30.,
                //             world_size: WORLD_SIZE,
                //             fog_of_war_texture: fog_of_war_texture.handle.clone(),
                //         },
                //     }),
                //     transform: Transform::from_translation(Vec3::new(0., 0., 0.)),
                //     ..default()
                // })
                // .insert(Collider::cuboid(WORLD_SIZE / 2., 0.1, WORLD_SIZE / 2.))
                // .insert(NavMeshAffector)
                // .insert(Terrain);
            }
        }
    }
}

pub fn ground_line_highlighter(
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TerrainMaterialExtension>>>,
    highlighter_q: Query<&Handle<ExtendedMaterial<StandardMaterial, TerrainMaterialExtension>>>,
    line_holders_q: Query<&LineHolder, Without<CircleHolder>>,
    circle_holders_q: Query<&CircleHolder, Without<LineHolder>>,
    timer: Res<Time>,
    mut elapsed_time: Local<u128>,
){
    *elapsed_time += timer.delta().as_millis();
    if *elapsed_time >= 250 {
        *elapsed_time = 0;

        if line_holders_q.is_empty() {
            for handle in &highlighter_q {
                if let Some(material) = materials.get_mut(handle) {
                    material.extension.lines = vec![];
                    material.extension.line_count = 0;
                }
            }
        } else {
            let mut lines: Vec<LineData> = Vec::new();

            for line_holder in line_holders_q.iter() {
                lines.extend(line_holder.0.iter());
            }

            for handle in &highlighter_q {
                if let Some(material) = materials.get_mut(handle) {
                    material.extension.lines = lines.clone();
                    material.extension.line_count = lines.len() as u32;
                }
            }
        }

        if circle_holders_q.is_empty() {
            for handle in &highlighter_q {
                if let Some(material) = materials.get_mut(handle) {
                    material.extension.circles = vec![];
                    material.extension.circle_count = 0;
                }
            }
        } else {
            let mut circles: Vec<CircleData> = Vec::new();

            for circle_holder in circle_holders_q.iter() {
                circles.extend(circle_holder.0.iter());
            }

            for handle in &highlighter_q {
                if let Some(material) = materials.get_mut(handle) {
                    material.extension.circles = circles.clone();
                    material.extension.circle_count = circles.len() as u32;
                }
            }
        }
    }
}

#[derive(Component)]
pub struct ForbiddenBlueprint;

pub fn blueprint_placement_color_definer (
    mut commands: Commands,
    blueprints_q: Query<(Entity, Option<&ForbiddenBlueprint>), With<DisplayedModelHolder>>,
    instanced_materials: Res<InstancedMaterials>,
){
    for blueprint in blueprints_q.iter() {
        if let Some(_) = blueprint.1 {
            commands.entity(blueprint.0).insert(instanced_materials.red_transparent.clone());
        } else {
            commands.entity(blueprint.0).insert(instanced_materials.blue_transparent.clone());
        }
    }
}

#[derive(Resource)]
pub struct InstancedMaterials{
    pub team_material: HashMap<(AssetId<Mesh>, i32), Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    pub blue_solid: Handle<StandardMaterial>,
    pub red_solid: Handle<StandardMaterial>,
    pub blue_transparent: Handle<StandardMaterial>,
    pub red_transparent: Handle<StandardMaterial>,
}

#[derive(Component, Clone)]
pub struct LOD{
    pub detailed: (Handle<Mesh>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>),
    pub simplified: PbrBundle,
}

const LOD_SWITCH_HEIGHT: f32 = 500.;

pub fn lod_system(
    camera_q: Query<&Transform, With<CameraComponent>>,
    mut lods_q: Query<(Entity, &LOD, &CombatComponent), (With<LOD>, With<CombatComponent>)>,
    mut child_lods_q: Query<(Entity, &Parent, &LOD), (Without<CombatComponent>, With<LOD>)>,
    mut commands: Commands,
    instanced_materials: Res<InstancedMaterials>,
    mut less: Local<bool>,
    mut more: Local<bool>,
){
    let camera = camera_q.single();

    if camera.translation.y < LOD_SWITCH_HEIGHT {
        if !*less {
            *less = true;
            *more = false;

            for model in lods_q.iter_mut() {
                commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();

                commands.entity(model.0).insert((
                    model.1.detailed.0.clone(),
                    model.1.detailed.1.clone(),
                ));
            }

            for model in child_lods_q.iter_mut() {
                commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();

                commands.entity(model.0).insert((
                    model.2.detailed.0.clone(),
                    model.2.detailed.1.clone(),
                ));
            }
        }
    } else if !*more {
        *less = false;
        *more = true;

        for model in lods_q.iter_mut() {
            let mat;

            if model.2.team == 1 {
                mat = instanced_materials.blue_solid.clone();
            } else {
                mat = instanced_materials.red_solid.clone();
            }

            commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();
            
            commands.entity(model.0).insert((
                model.1.simplified.mesh.clone(),
                mat,
            ));
        }

        for model in child_lods_q.iter_mut() {
            if let Ok(parent) = lods_q.get(**model.1) {
                let mat;

                if parent.2.team == 1 {
                    mat = instanced_materials.blue_solid.clone();
                } else {
                    mat = instanced_materials.red_solid.clone();
                }

                commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();

                commands.entity(model.0).insert((
                    model.2.simplified.mesh.clone(),
                    mat,
                ));
            }
        }
    }
}

#[derive(Component)]
pub struct TrailEmmiterComponent;

#[derive(Component)]
pub struct TrailComponent {
    pub positions: Vec<Vec3>,
    pub length: usize,
    pub width: f32,
    pub mesh_handle: Handle<Mesh>,
    pub emmiter_entity: Entity,
}

pub fn trail_processing_system (
    trail_emmiters_q: Query<&GlobalTransform, With<TrailEmmiterComponent>>,
    mut trails_q: Query<(Entity, &mut Transform, &mut TrailComponent), With<TrailComponent>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    time: Res<Time>,
    mut update_elapsed: Local<u128>,
){
    *update_elapsed += time.delta().as_millis();
    if *update_elapsed >= 25 {
        *update_elapsed = 0;
    }

    for mut trail in trails_q.iter_mut() {
        if let Ok(emmiter_transform) = trail_emmiters_q.get(trail.2.emmiter_entity) {
            trail.1.translation = emmiter_transform.translation();

            if *update_elapsed == 0 {
                trail.2.positions.push(trail.1.translation);
                
                if trail.2.positions.len() > trail.2.length {
                    trail.2.positions.remove(0);
                }
                
                if let Some(mesh) = meshes.get_mut(&trail.2.mesh_handle) {
                    *mesh = generate_trail_mesh(trail.2.positions.clone(), trail.2.width, &trail.1);
                }
            }
        } else {
            commands.entity(trail.0).despawn();
        }
    }
}

fn generate_trail_mesh(positions: Vec<Vec3>, width: f32, object_transform: &Transform) -> Mesh {
    if positions.len() < 2 {
        return Mesh::from(Triangle3d{
            vertices: [Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
        });
    }

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    let object_matrix = object_transform.compute_matrix();
    let inverse_matrix = object_matrix.inverse();

    for i in 0..positions.len() - 1 {
        let p0 = inverse_matrix.transform_point3(positions[i]);
        let p1 = inverse_matrix.transform_point3(positions[i + 1]);

        let direction = (p1 - p0).normalize_or_zero();

        let up = Vec3::Y;
        let right = direction.cross(up).normalize_or_zero();

        if right == Vec3::ZERO {
            continue;
        }

        let half_width = width * 0.5;
        let offset = right * half_width;

        let v0 = p0 - offset;
        let v1 = p0 + offset;
        let v2 = p1 - offset;
        let v3 = p1 + offset;

        vertices.push(v0);
        vertices.push(v1);
        vertices.push(v2);
        vertices.push(v3);

        normals.push(up);
        normals.push(up);
        normals.push(up);
        normals.push(up);

        let u0 = i as f32 / (positions.len() as f32 - 1.0);
        let u1 = (i + 1) as f32 / (positions.len() as f32 - 1.0);
        uvs.push([u0, 0.0]);
        uvs.push([u0, 1.0]);
        uvs.push([u1, 0.0]);
        uvs.push([u1, 1.0]);

        let base_idx = (i * 4) as u32;
        indices.extend_from_slice(&[
            base_idx + 0,
            base_idx + 1,
            base_idx + 2,
            base_idx + 2,
            base_idx + 1,
            base_idx + 3,
        ]);
    }

    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        RenderAssetUsages::all(),
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));

    mesh
}