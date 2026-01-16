use std::{f32::consts::TAU, primitive};

use bevy::{color::palettes::css::GRAY, gltf::GltfMesh, pbr::{ExtendedMaterial, MaterialExtension, NotShadowCaster}, prelude::*, render::{mesh::{Indices, MeshVertexBufferLayout, skinning::{SkinnedMesh, SkinnedMeshInverseBindposes}}, render_asset::RenderAssetUsages, render_resource::{AsBindGroup, DynamicUniformBuffer, PipelineDescriptor, RenderPipelineDescriptor, Sampler, ShaderRef, ShaderType, SpecializedMeshPipelineError}, texture::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor}}, scene::SceneInstance, transform::commands, utils::{HashSet, hashbrown::HashMap}};
use bevy_rapier3d::{na::TGeneral, prelude::{Collider, CollisionGroups, ComputedColliderShape, Group}};
use oxidized_navigation_serializable::NavMeshAffector;

use crate::{WORLD_SIZE, components::{building::{BuildingBlueprint, UnactivatedBlueprints}, camera::CameraComponent, ui_manager::DisplayedModelHolder, unit::{AttackTypes, CombatComponent, CompanyTypes, FogOfWarTexture, NeedToMove, StoppedMoving, UnitComponent, UnitTypes}}};

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
pub struct UnitAssets {
    pub regular_soldier: (Handle<Scene>, Vec<Handle<AnimationClip>>),
    pub assault_soldier: (Handle<Scene>, Vec<Handle<AnimationClip>>),
    pub atgm_soldier: (Handle<Scene>, Vec<Handle<AnimationClip>>),
    pub rpg_soldier: (Handle<Scene>, Vec<Handle<AnimationClip>>),
    pub sniper_soldier: (Handle<Scene>, Vec<Handle<AnimationClip>>),
    pub corpse: (Handle<Mesh>, Handle<StandardMaterial>),
    pub tank: (Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>),
    pub ifv: (Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>),
    pub artillery: (Handle<Mesh>, Handle<StandardMaterial>),
    pub truck: (Handle<Mesh>, Handle<StandardMaterial>, Handle<Mesh>),
    pub engineer: (Handle<Mesh>, Handle<StandardMaterial>),

    pub infantry_simplified_mesh: Handle<Mesh>,
    pub vehicle_simplified_mesh: Handle<Mesh>,
    pub corpse_simplified_mesh: Handle<Mesh>,
}

#[derive(Resource)]
pub struct OtherAssets {
    pub regular_infantry_squad_symbol_blufor: Handle<Image>,
    pub shock_infantry_squad_symbol_blufor: Handle<Image>,
    pub armored_squad_symbol_blufor: Handle<Image>,
    pub artillery_unit_symbol_blufor: Handle<Image>,
    pub engineer_unit_symbol_blufor: Handle<Image>,

    pub regular_infantry_platoon_symbol_blufor: Handle<Image>,
    pub shock_infantry_platoon_symbol_blufor: Handle<Image>,
    pub armored_platoon_symbol_blufor: Handle<Image>,

    pub regular_infantry_company_symbol_blufor: Handle<Image>,
    pub shock_infantry_company_symbol_blufor: Handle<Image>,
    pub armored_company_symbol_blufor: Handle<Image>,

    pub battalion_symbol_blufor: Handle<Image>,

    pub regiment_symbol_blufor: Handle<Image>,

    pub brigade_symbol_blufor: Handle<Image>,



    pub regular_infantry_squad_symbol_opfor: Handle<Image>,
    pub shock_infantry_squad_symbol_opfor: Handle<Image>,
    pub armored_squad_symbol_opfor: Handle<Image>,
    pub artillery_unit_symbol_opfor: Handle<Image>,
    pub engineer_unit_symbol_opfor: Handle<Image>,

    pub regular_infantry_platoon_symbol_opfor: Handle<Image>,
    pub shock_infantry_platoon_symbol_opfor: Handle<Image>,
    pub armored_platoon_symbol_opfor: Handle<Image>,

    pub regular_infantry_company_symbol_opfor: Handle<Image>,
    pub shock_infantry_company_symbol_opfor: Handle<Image>,
    pub armored_company_symbol_opfor: Handle<Image>,

    pub battalion_symbol_opfor: Handle<Image>,

    pub regiment_symbol_opfor: Handle<Image>,

    pub brigade_symbol_opfor: Handle<Image>,



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

    pub explosion_regular: (Handle<StandardMaterial>, Vec<Handle<Mesh>>),
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
    let regular_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier.glb"));
    let regular_soldier_animation1: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier.glb"));
    let regular_soldier_animation2: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(1).from_asset("units/soldier.glb"));

    let assault_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_assault.glb"));
    let assault_soldier_animation1: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_assault.glb"));
    let assault_soldier_animation2: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(1).from_asset("units/soldier_assault.glb"));

    let atgm_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_atgm.glb"));
    let atgm_soldier_animation1: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_atgm.glb"));
    let atgm_soldier_animation2: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(1).from_asset("units/soldier_atgm.glb"));
    

    let rpg_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_rpg.glb"));
    let rpg_soldier_animation1: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_rpg.glb"));
    let rpg_soldier_animation2: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(1).from_asset("units/soldier_rpg.glb"));

    let sniper_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_sniper.glb"));
    let sniper_soldier_animation1: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_sniper.glb"));
    let sniper_soldier_animation2: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(1).from_asset("units/soldier_sniper.glb"));

    let corpse_mesh: Handle<Mesh> = asset_server.load("units/corpse.glb#Mesh0/Primitive0");
    let corpse_material: Handle<StandardMaterial> = asset_server.load("units/corpse.glb#Material0");

    // let regular_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier.glb"));
    // let regular_soldier_animation: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier.glb"));

    // let assault_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_assault.glb"));
    // let assault_soldier_animation: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_assault.glb"));

    // let atgm_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_atgm.glb"));
    // let atgm_soldier_animation: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_atgm.glb"));

    // let rpg_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_rpg.glb"));
    // let rpg_soldier_animation: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_rpg.glb"));

    // let sniper_soldier_scene: Handle<Scene> = asset_server.load(GltfAssetLabel::Scene(0).from_asset("units/soldier_sniper.glb"));
    // let sniper_soldier_animation: Handle<AnimationClip> = asset_server.load(GltfAssetLabel::Animation(0).from_asset("units/soldier_sniper.glb"));

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
    let truck_simplified_mesh = meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 1.5, 4.) }.mesh()));

    let engineer_mesh: Handle<Mesh> = asset_server.load("units/engineer.glb#Mesh0/Primitive0");
    let engineer_material: Handle<StandardMaterial> = asset_server.load("units/engineer.glb#Material0");

    let infantry_simplified_mesh = meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(0.5, 1., 0.5) }.mesh()));
    let vehicle_simplified_mesh = meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(2., 1.5, 4.) }.mesh()));
    let corpse_simplified_mesh = meshes.add(Mesh::from(Cuboid{ half_size: Vec3::new(0.5, 0.5, 1.) }.mesh()));

    commands.insert_resource(UnitAssets{
        regular_soldier: (regular_soldier_scene, vec![regular_soldier_animation1, regular_soldier_animation2]),
        assault_soldier: (assault_soldier_scene, vec![assault_soldier_animation1, assault_soldier_animation2]),
        atgm_soldier: (atgm_soldier_scene, vec![atgm_soldier_animation1, atgm_soldier_animation2]),
        rpg_soldier: (rpg_soldier_scene, vec![rpg_soldier_animation1, rpg_soldier_animation2]),
        sniper_soldier: (sniper_soldier_scene, vec![sniper_soldier_animation1, sniper_soldier_animation2]),
        corpse: (corpse_mesh, corpse_material),
        tank: (tank_mesh_hull, tank_mesh_turret, tank_material),
        ifv: (ifv_mesh_hull, ifv_mesh_turret, ifv_material),
        artillery: (artillery_mesh, artillery_material),
        truck: (truck_mesh, truck_material, truck_simplified_mesh),
        engineer: (engineer_mesh, engineer_material),

        infantry_simplified_mesh: infantry_simplified_mesh,
        vehicle_simplified_mesh: vehicle_simplified_mesh,
        corpse_simplified_mesh: corpse_simplified_mesh,
    });
    //Units^

    //Other
    let regular_infantry_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/regular_infantry_squad.png");
    let shock_infantry_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/shock_infantry_squad.png");
    let armored_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/armored_squad.png");
    let artillery_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/artillery_unit.png");
    let engineer_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/engineer_unit.png");

    let regular_infantry_platoon_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/regular_infantry_platoon.png");
    let shock_infantry_platoon_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/shock_infantry_platoon.png");
    let armored_platoon_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/armored_platoon.png");

    let regular_infantry_company_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/regular_infantry_company.png");
    let shock_infantry_company_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/shock_infantry_company.png");
    let armored_company_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/armored_company.png");

    let battalion_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/battalion.png");

    let regiment_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/regiment.png");

    let brigade_symbol_blufor: Handle<Image> = asset_server.load("icons/tactical/blufor/brigade.png");



    let regular_infantry_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/regular_infantry_squad.png");
    let shock_infantry_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/shock_infantry_squad.png");
    let armored_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/armored_squad.png");
    let artillery_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/artillery_unit.png");
    let engineer_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/engineer_unit.png");

    let regular_infantry_platoon_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/regular_infantry_platoon.png");
    let shock_infantry_platoon_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/shock_infantry_platoon.png");
    let armored_platoon_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/armored_platoon.png");

    let regular_infantry_company_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/regular_infantry_company.png");
    let shock_infantry_company_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/shock_infantry_company.png");
    let armored_company_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/armored_company.png");

    let battalion_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/battalion.png");

    let regiment_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/regiment.png");

    let brigade_symbol_opfor: Handle<Image> = asset_server.load("icons/tactical/opfor/brigade.png");



    let materials_icon: Handle<Image> = asset_server.load("icons/resources/materials.png");

    let human_resource_icon: Handle<Image> = asset_server.load("icons/resources/human.png");

    commands.insert_resource(OtherAssets{
        regular_infantry_squad_symbol_blufor: regular_infantry_symbol_blufor,
        shock_infantry_squad_symbol_blufor: shock_infantry_symbol_blufor,
        armored_squad_symbol_blufor: armored_symbol_blufor,
        artillery_unit_symbol_blufor: artillery_symbol_blufor,
        engineer_unit_symbol_blufor: engineer_symbol_blufor,

        regular_infantry_platoon_symbol_blufor: regular_infantry_platoon_symbol_blufor,
        shock_infantry_platoon_symbol_blufor: shock_infantry_platoon_symbol_blufor,
        armored_platoon_symbol_blufor: armored_platoon_symbol_blufor,

        regular_infantry_company_symbol_blufor: regular_infantry_company_symbol_blufor,
        shock_infantry_company_symbol_blufor: shock_infantry_company_symbol_blufor,
        armored_company_symbol_blufor: armored_company_symbol_blufor,

        battalion_symbol_blufor: battalion_symbol_blufor,

        regiment_symbol_blufor: regiment_symbol_blufor,

        brigade_symbol_blufor: brigade_symbol_blufor,



        regular_infantry_squad_symbol_opfor: regular_infantry_symbol_opfor,
        shock_infantry_squad_symbol_opfor: shock_infantry_symbol_opfor,
        armored_squad_symbol_opfor: armored_symbol_opfor,
        artillery_unit_symbol_opfor: artillery_symbol_opfor,
        engineer_unit_symbol_opfor: engineer_symbol_opfor,

        regular_infantry_platoon_symbol_opfor: regular_infantry_platoon_symbol_opfor,
        shock_infantry_platoon_symbol_opfor: shock_infantry_platoon_symbol_opfor,
        armored_platoon_symbol_opfor: armored_platoon_symbol_opfor,

        regular_infantry_company_symbol_opfor: regular_infantry_company_symbol_opfor,
        shock_infantry_company_symbol_opfor: shock_infantry_company_symbol_opfor,
        armored_company_symbol_opfor: armored_company_symbol_opfor,

        battalion_symbol_opfor: battalion_symbol_opfor,

        regiment_symbol_opfor: regiment_symbol_opfor,

        brigade_symbol_opfor: brigade_symbol_opfor,



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

    let explosion_atlas: Handle<Image> = asset_server.load("textures/explosions/explosion_atlas.png");

    let explosion_material = materials.add(StandardMaterial{
        base_color_texture: Some(explosion_atlas),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default() 
    });

    let mut explosion_frame_meshes: Vec<Handle<Mesh>> = Vec::new();

    for i in 0..48 {
        explosion_frame_meshes.push(
            meshes.add(atlas_mesh_frame_generator(
                i,
                2000,
                1500,
                250,
                250,
                50.,
                50.,
            ))
        );
    }

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
        explosion_regular: (explosion_material, explosion_frame_meshes),
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
                .insert(Terrain)
                .insert(CollisionGroups::new(Group::GROUP_10, Group::all()));

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
            commands.entity(blueprint.0).try_insert(instanced_materials.red_transparent.clone());
        } else {
            commands.entity(blueprint.0).try_insert(instanced_materials.blue_transparent.clone());
        }
    }
}

#[derive(Resource)]
pub struct InstancedMaterials{
    pub team_materials: HashMap<(AssetId<Mesh>, i32), Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    pub blue_solid: Handle<StandardMaterial>,
    pub red_solid: Handle<StandardMaterial>,
    pub blue_transparent: Handle<StandardMaterial>,
    pub red_transparent: Handle<StandardMaterial>,
    pub wreck_material: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub struct InstancedAnimations {
    pub running_animations: HashMap<String, (Vec<AnimationNodeIndex>, Handle<AnimationGraph>)>,
}

#[derive(Component, Clone)]
pub struct LOD{
    pub detailed: (Handle<Mesh>, Option<Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>, Option<Handle<StandardMaterial>>),
    pub simplified: (Handle<Mesh>, Handle<StandardMaterial>),
}

const LOD_SWITCH_HEIGHT: f32 = 300.;

pub fn lod_system(
    camera_q: Query<&Transform, With<CameraComponent>>,
    mut lods_q: Query<(Entity, &LOD, Option<&mut AnimatedMesh>, Option<&SkinnedMesh>), (With<LOD>, Without<AnimationComponent>, Without<ChangeMaterial>)>,
    mut commands: Commands,
    mut less: Local<bool>,
    mut more: Local<bool>,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
){
    if lods_q.is_empty() {return;}

    if *elapsed_time < 1000 {
        *elapsed_time += time.delta().as_millis();
        return;
    }
    
    let camera = camera_q.single();

    if camera.translation.y < LOD_SWITCH_HEIGHT {
        if !*less {
            *less = true;
            *more = false;

            for model in lods_q.iter_mut() {
                commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();

                if let Some(animated_mesh) = model.2 {
                    if !animated_mesh.joints.is_empty() {
                        commands.entity(model.0).try_insert(SkinnedMesh{
                            inverse_bindposes: animated_mesh.inverse_bindposes.clone(),
                            joints: animated_mesh.joints.clone(),
                        });
                    }
                }

                commands.entity(model.0).try_insert(model.1.detailed.0.clone());

                if let Some(team_material) = &model.1.detailed.1 {
                    commands.entity(model.0).try_insert(team_material.clone());
                } else if let Some(simple_material) = &model.1.detailed.2 {
                    commands.entity(model.0).try_insert(simple_material.clone());
                }
            }
        }
    } else if !*more {
        *less = false;
        *more = true;

        for model in lods_q.iter_mut() {
            commands.entity(model.0).remove::<(Handle<Mesh>, Handle<StandardMaterial>, Handle<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>)>();

            if let Some(skinned_mesh) = model.3 {
                if let Some(mut animated_mesh) = model.2 {
                    animated_mesh.inverse_bindposes = skinned_mesh.inverse_bindposes.clone();
                    animated_mesh.joints = skinned_mesh.joints.clone();

                    commands.entity(model.0).remove::<SkinnedMesh>();
                }
            }

            commands.entity(model.0).try_insert(model.1.simplified.0.clone());
            commands.entity(model.0).try_insert(model.1.simplified.1.clone());
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

#[derive(Component, Clone)]
pub struct ChangeMaterial;

pub fn testing_system(
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    team_materials: Res<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    time: Res<Time>,
    mut elapsed_time: Local<u128>,
) {
    *elapsed_time += time.delta().as_millis();

    if *elapsed_time >= 1000 {
        *elapsed_time = 0;

        // println!("meshes: {}", meshes.iter().count());
        // println!("materials: {}", materials.iter().count());
        // println!("team_materials: {}", team_materials.iter().count());
        // println!("=================================================");
    }
}

#[derive(Component)]
pub struct AnimatedMesh{
    pub inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    pub joints: Vec<Entity>,
}

pub fn apply_team_material_to_scenes (
    scenes_q: Query<(Entity, &CombatComponent, &LOD), Added<ChangeMaterial>>,
    children_q: Query<&Children>,
    mesh_material_q: Query<(&Handle<Mesh>, &Handle<StandardMaterial>, Option<&LOD>)>,
    mut instanced_materials: ResMut<InstancedMaterials>,
    materials: Res<Assets<StandardMaterial>>,
    mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    mut commands: Commands,
) {
    for scene in scenes_q.iter() {
        let mut parents = vec![scene.0];
        let mut is_mesh_material_found = false;

        loop {
            let mut new_parents = Vec::new();
            for parent in parents.iter() {
                if is_mesh_material_found {
                    new_parents.clear();
                    break;
                }

                if let Ok(children) = children_q.get(*parent) {
                    for child in children.iter() {
                        new_parents.push(*child);

                        if let Ok(mesh_material) = mesh_material_q.get(*child) {
                            let color;

                            match scene.1.team {
                                1 => {
                                    color = Vec4::new(0., 0., 1., 1.);
                                }
                                2 => {
                                    color = Vec4::new(1., 0., 0., 1.);
                                }
                                _ => {
                                    color = Vec4::new(1., 1., 1., 1.);
                                }
                            }

                            let material;

                            if let Some(mat) = instanced_materials.team_materials.get(&(mesh_material.0.id(), scene.1.team)) {
                                material = mat.clone();
                            } else {
                                if let Some(original) = materials.get(mesh_material.1.id()) {
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

                                instanced_materials.team_materials.insert((mesh_material.0.id(), scene.1.team), material.clone());
                            }

                            commands.entity(*child).remove::<Handle<StandardMaterial>>();
                            commands.entity(*child).insert(material.clone());
                            commands.entity(*child).remove::<ChangeMaterial>();

                            if mesh_material.2.is_none() {
                                commands.entity(*child).insert((
                                    LOD{
                                        detailed: (mesh_material.0.clone(), Some(material), None),
                                        simplified: (
                                            scene.2.simplified.0.clone(),
                                            scene.2.simplified.1.clone(),
                                        ),
                                    },
                                    AnimatedMesh{
                                        inverse_bindposes: Handle::default(),
                                        joints: Vec::new(),
                                    },
                                ));
                            }

                            is_mesh_material_found = true;
                            break;
                        }
                    }
                }
            }

            if new_parents.is_empty() {
                break;
            }

            parents = new_parents;
        }
    }
}

#[derive(Component, Clone)]
pub struct AnimationComponent(pub Vec<Handle<AnimationClip>>);

pub fn running_animation_manager(
    stopped_q: Query<(&CombatComponent, &AnimationComponent, Entity), Added<StoppedMoving>>,
    running_q: Query<(&CombatComponent, &AnimationComponent, Entity), Added<NeedToMove>>,
    mut animation_players_q: Query<(Entity, &mut AnimationPlayer)>,
    children_q: Query<&Children>,
    mut instanced_animations: ResMut<InstancedAnimations>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut commands: Commands,
) {
    for runner in stopped_q.iter() {
        let mut parents = vec![runner.2];
        let mut is_animation_found = false;

        commands.entity(runner.2).remove::<StoppedMoving>();

        loop {
            let mut new_parents = Vec::new();

            for parent in parents.iter() {
                if is_animation_found {
                    new_parents.clear();
                    break;
                }

                if let Ok(children) = children_q.get(*parent) {
                    for child in children.iter() {
                        new_parents.push(*child);

                        if let Ok(mut animation_player) = animation_players_q.get_mut(*child) {
                            if let Some(animation) = instanced_animations.running_animations.get_mut(&runner.0.unit_data.1.2) {
                                let mut transitions = AnimationTransitions::new();

                                transitions
                                .play(&mut animation_player.1, animation.0[0], std::time::Duration::ZERO)
                                .repeat();

                                commands
                                .entity(animation_player.0)
                                .try_insert(animation.1.clone())
                                .try_insert(transitions);

                                animation_player.1.stop(animation.0[1]);
                                animation_player.1.play(animation.0[0]);
                            } else {
                                let mut graph = AnimationGraph::new();

                                let animation_indices: Vec<AnimationNodeIndex> = graph
                                    .add_clips(runner.1.0.clone(), 1.0, graph.root)
                                    .collect();

                                let graph_handle = graphs.add(graph);

                                instanced_animations.running_animations.try_insert(runner.0.unit_data.1.2.clone(), (animation_indices.clone(), graph_handle.clone()));

                                let mut transitions = AnimationTransitions::new();

                                transitions
                                .play(&mut animation_player.1, animation_indices[0], std::time::Duration::ZERO)
                                .repeat();

                                commands
                                .entity(animation_player.0)
                                .try_insert(graph_handle)
                                .try_insert(transitions);

                                animation_player.1.stop(animation_indices[1]);
                                animation_player.1.play(animation_indices[0]);
                            }

                            is_animation_found = true;
                            break;
                        }
                    }
                }
            }

            if new_parents.is_empty() {
                break;
            }

            parents = new_parents;
        }
    }

    for runner in running_q.iter() {
        let mut parents = vec![runner.2];
        let mut is_animation_found = false;

        loop {
            let mut new_parents = Vec::new();

            for parent in parents.iter() {
                if is_animation_found {
                    new_parents.clear();
                    break;
                }

                if let Ok(children) = children_q.get(*parent) {
                    for child in children.iter() {
                        new_parents.push(*child);

                        if let Ok(mut animation_player) = animation_players_q.get_mut(*child) {
                            if let Some(animation) = instanced_animations.running_animations.get_mut(&runner.0.unit_data.1.2) {
                                let mut transitions = AnimationTransitions::new();

                                transitions
                                .play(&mut animation_player.1, animation.0[1], std::time::Duration::ZERO)
                                .repeat();

                                commands
                                .entity(animation_player.0)
                                .try_insert(animation.1.clone())
                                .try_insert(transitions);

                                animation_player.1.stop(animation.0[0]);
                                animation_player.1.play(animation.0[1]).repeat().set_speed(2.);
                            } else {
                                let mut graph = AnimationGraph::new();

                                let animation_indices: Vec<AnimationNodeIndex> = graph
                                    .add_clips(runner.1.0.clone(), 1.0, graph.root)
                                    .collect();

                                let graph_handle = graphs.add(graph);

                                instanced_animations.running_animations.try_insert(runner.0.unit_data.1.2.clone(), (animation_indices.clone(), graph_handle.clone()));

                                let mut transitions = AnimationTransitions::new();

                                transitions
                                .play(&mut animation_player.1, animation_indices[1], std::time::Duration::ZERO)
                                .repeat();

                                commands
                                .entity(animation_player.0)
                                .try_insert(graph_handle)
                                .try_insert(transitions);

                                animation_player.1.stop(animation_indices[0]);
                                animation_player.1.play(animation_indices[1]).repeat().set_speed(2.);
                            }

                            is_animation_found = true;
                            break;
                        }
                    }
                }
            }

            if new_parents.is_empty() {
                break;
            }

            parents = new_parents;
        }
    }
}

pub fn atlas_mesh_frame_generator(
    frame_index: usize,
    atlas_width_px: u32,
    atlas_height_px: u32,
    frame_width_px: u32,
    frame_height_px: u32,
    mesh_width: f32,
    mesh_height: f32,
) -> Mesh {
    let cols = (atlas_width_px / frame_width_px) as usize;
    let rows = (atlas_height_px / frame_height_px) as usize;

    let row = frame_index / cols;
    let col = frame_index - cols * row;

    let row_inverted = (rows - 1) - row;

    let frame_x = col as u32 * frame_width_px;
    let frame_y = row_inverted as u32 * frame_height_px;

    let atlas_w = atlas_width_px as f32;
    let atlas_h = atlas_height_px as f32;
    let fw = frame_width_px as f32;
    let fh = frame_height_px as f32;
    let fx = frame_x as f32;
    let fy = frame_y as f32;

    let u1 = fx / atlas_w;
    let u0 = (fx + fw) / atlas_w;
    let v1 = 1.0 - (fy + fh) / atlas_h;
    let v0 = 1.0 - fy / atlas_h;

    let half_w = mesh_width * 0.5;
    let half_h = mesh_height * 0.5;

    let positions = vec![
        [-half_w, -half_h, 0.0],
        [ half_w, -half_h, 0.0],
        [ half_w,  half_h, 0.0],
        [-half_w,  half_h, 0.0],
    ];

    let uvs = vec![
        [u0, v0],
        [u1, v0],
        [u1, v1],
        [u0, v1],
    ];

    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, -1.0]; 4]);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));

    mesh
}

#[derive(Component)]
pub struct ExplosionComponent(pub (usize, u128));

pub fn explosion_effects_handler(
    mut explosions_q: Query<(Entity, &mut ExplosionComponent, &mut Transform), Without<CameraComponent>>,
    camera_q: Query<&Transform, With<CameraComponent>>,
    assets: Res<AttackVisualisationAssets>,
    mut commands: Commands,
    time: Res<Time>,
){
    let camera_pos = camera_q.single().translation;

    for mut explosion in explosions_q.iter_mut() {
        explosion.2.look_at(camera_pos, Vec3::Y);

        explosion.1.0.1 += time.delta().as_millis();

        if explosion.1.0.1 > 20 {
            explosion.1.0.1 = 0;
            explosion.1.0.0 += 1;

            if explosion.1.0.0 >= assets.explosion_regular.1.len() {
                commands.entity(explosion.0).despawn();

                continue;
            }

            commands.entity(explosion.0).insert(assets.explosion_regular.1[explosion.1.0.0].clone());
        }
    }
}