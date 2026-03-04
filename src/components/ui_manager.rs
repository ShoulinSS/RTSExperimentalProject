use std::{clone, default, f32::consts::E, string, thread::current};
use bevy::{log::tracing_subscriber::fmt::format, math::VectorSpace, pbr::{ExtendedMaterial, NotShadowCaster}, prelude::*, transform::commands, ui::{self, AvailableSpace}, utils::hashbrown::{HashMap, HashSet}, window::PrimaryWindow};
use bevy_egui::{EguiContext, EguiContexts, EguiRenderToTextureHandle, EguiUserTextures, egui::{self, Color32, Context, FontId, Stroke}};
use bevy_mod_raycast::{cursor::{self, CursorRay}, prelude::{Raycast, RaycastSettings}};
use bevy_quinnet::{client::QuinnetClient, server::QuinnetServer};
use bevy_rapier3d::{plugin::RapierContext, prelude::{Collider, QueryFilter}, rapier::crossbeam::epoch::CompareAndSetOrdering};
use bevy_tasks::TaskPool;
use oxidized_navigation_serializable::NavMeshAffector;

use crate::{GameStage, GameStages, GameState, HUMAN_RESOURCE_COLOR, MATERIALS_COLOR, PlayerData, SUPPLIES_COLOR, components::{self, asset_manager::{CircleData, CircleHolder, ForbiddenBlueprint, InstancedAnimations, InstancedMaterials, OtherAssets, TeamMaterialExtension, Terrain}, building::{BuildingStageCache, BuildingsDeletionStates, HumanResourceStorageComponent, MaterialsProductionComponent, MaterialsStorageComponent, ProducableUnits, SettlementComponent, Settlements, SettlementsLeft, SwitchableBuilding, VILLAGES_COUNT}, camera::SelectionBox, network::{EntityMaps, UnitsToDamage, UnitsToInsertPath, UnspecifiedEntitiesToMove}, unit::{ARMY_SIZE, AsyncTaskPools, BATTALION_SIZE, COMPANY_SIZE, DisabledUnit, InfantryTransport, IsUnitDeselectionAllowed, IsUnitSelectionAllowed, PLATOON_SIZE, REGIMENT_SIZE, RemainsCount, START_ARTILLERY_UNITS_COUNT, SuppliesConsumerComponent}}};

use super::{asset_manager::{generate_circle_segments, LineData, LineHolder}, building::{AllSettlementsPlaced, BuildingBlueprint, BuildingsBundles, BuildingsList, InfantryBarracksBundle, ProductionButtonPressed, ProductionQueue, ProductionState, SoldierBundle, UnactivatedBlueprints, UnitBundles, VehicleFactoryBundle, ALLOWED_DISTANCE_FROM_BORDERS, CITIES_COUNT}, camera::{CameraComponent, SelectionBounds}, logistics::ResourceZone, network::{ClientList, ClientMessage, InsertedConnectionData, NetworkStatus, NetworkStatuses, PlayerList, ServerMessage}, unit::{self, Armies, ArmoredSquad, ArtilleryUnit, CompanyTypes, CombatComponent, IsArtilleryDesignationActive, LimitedHashMap, LimitedHashSet, LimitedNumber, SquadLeader, RegularSquad, SelectedUnit, SerializableArmoredSquad, SerializableArmyObject, SerializableRegularSquad, SerializableShockSquad, ShockSquad, UnitTypes, UnitsTileMap, MAX_SQUAD_COUNT, START_ARMORED_SQUADS_AMOUNT, START_REGULAR_SQUADS_AMOUNT, START_SHOCK_SQUADS_AMOUNT, TILE_SIZE}};

pub enum Actions {
    OpenArmySettings,
    OpenCompanyTypes((i32, (i32, i32, i32))),
    ChooseCompanyType((CompanyTypes, (i32, i32, i32), i32)),
    SetupCompany((i32, (i32, i32, i32))),
    OpenSquadSpecializations((i32, (i32, i32, i32, i32, i32), CompanyTypes)),
    ChooseSquadSpecialization(((String, String), (i32, i32, i32, i32, i32), i32, CompanyTypes)),
    SquadSelection((i32, (CompanyTypes, (i32, i32, i32 ,i32 ,i32)))),
    PlatoonSelection((i32, (CompanyTypes, Vec<(i32, i32, i32 ,i32 ,i32)>))),
    CompanySelection((i32, (CompanyTypes, Vec<(i32, i32, i32 ,i32 ,i32)>))),
    BattalionSelection((i32, Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>)),
    RegimentSelection((i32, Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>)),
    BrigadeSelection((i32, Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>)),
    ToggleProduction,
    OpenBuildingsList,
    BuildingToBuildSelected((BuildingsBundles, Collider, f32, i32, String, f32, i32)),
    ToggleArtilleryDesignation,
    CancelArtilleryTargets,
    CompleteConstruction,
    OpenTacticalSymbolsLevels,
    ChangeTacticalSymbolsLevel(i32),
    ActivateBlueprintsDeletionMode,
    ActivateBuildingsDeletionMode,
    ActivateBuildingsDeletionCancelationMode,
    SwitchBuildingState(Entity),
    RebuildApartments(Entity),
    DisembarkInfantry,
    ArtilleryUnitSelection((i32, i32)),
    SwipeRegiment(i32),
}

#[derive(Event)]
pub struct LandArmyButtonClickEvent;

#[derive(Event)]
pub struct OpenCompanyTypesEvent((i32, (i32, i32, i32)));

#[derive(Event)]
pub struct ChooseCompanyTypeEvent((CompanyTypes, (i32, i32, i32), i32));

#[derive(Event)]
pub struct SetupCompanyEvent((i32, (i32, i32, i32)));

#[derive(Event)]
pub struct OpenSquadSpecializationsEvent((i32, (i32, i32, i32, i32, i32), CompanyTypes));

#[derive(Event)]
pub struct ChooseSquadSpecializationEvent(((String, String), (i32, i32, i32, i32, i32), i32, CompanyTypes));

#[derive(Event)]
pub struct ToggleProductionEvent;

#[derive(Event)]
pub struct ProductionStateChanged{
    pub team: i32,
    pub is_allowed: bool,
}

#[derive(Event)]
pub struct SquadSelectionEvent(pub (CompanyTypes, (i32, i32, i32 ,i32 ,i32)));

#[derive(Event)]
pub struct PlatoonSelectionEvent(pub (CompanyTypes, Vec<(i32, i32, i32 ,i32 ,i32)>));

#[derive(Event)]
pub struct CompanySelectionEvent(pub (CompanyTypes, Vec<(i32, i32, i32 ,i32 ,i32)>));

#[derive(Event)]
pub struct BattalionSelectionEvent(pub Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>);

#[derive(Event)]
pub struct RegimentSelectionEvent(pub Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>);

#[derive(Event)]
pub struct BrigadeSelectionEvent(pub Vec<(CompanyTypes, (i32, i32, i32 ,i32 ,i32))>);

#[derive(Event)]
pub struct OpenBuildingsListEvent;

#[derive(Event)]
pub struct BuildingToBuildSelectedEvent(pub (BuildingsBundles, Collider, f32, i32, String, f32, i32));

#[derive(Event)]
pub struct ToggleArtilleryDesignation;

#[derive(Event)]
pub struct CancelArtilleryTargets;

#[derive(Event)]
pub struct CompleteConstruction;

#[derive(Event)]
pub struct GameStartedEvent;

#[derive(Event)]
pub struct OpenTacticalSymbolsLevels;

#[derive(Event)]
pub struct ChangeTacticalSymbolsLevel(i32);

#[derive(Event)]
pub struct ActivateBlueprintsDeletionMode;

#[derive(Event)]
pub struct ActivateBuildingsDeletionMode;

#[derive(Event)]
pub struct ActivateBuildingsDeletionCancelationMode;

#[derive(Event)]
pub struct SwitchBuildingState(pub Entity);

#[derive(Event)]
pub struct RebuildApartments(pub Entity);

#[derive(Event)]
pub struct TransportDisembarkEvent;

#[derive(Event)]
pub struct ArtilleryUnitSelectedEvent(pub (i32, i32));

#[derive(Event)]
pub struct BuildingButtonHovered(pub String);

#[derive(Event)]
pub struct RegimentSwipeEvent(pub i32);

#[derive(Resource)]
pub struct ArmySettingsNodes {
    pub land_army_settings_node: Entity,
    pub land_army_settings_node_height: u32,
    pub land_army_settings_node_width: u32,
    pub is_land_army_settings_visible: bool,
    pub company_buttons: (i32, Entity, Vec<Entity>),
    pub last_battalion_button_index: i32,
    pub batallion_type_dropdown_lists: Vec<(Entity, CompanyTypes, LimitedNumber<0, 2>)>,
    pub last_battalion_type_dropdown_list_index: i32,
    pub platoon_specialization_dropdown_lists: Vec<(Entity, String, LimitedNumber<0, 2>)>,
    pub last_platoon_specialization_dropdown_list_index: i32,
    pub platoon_specialization_cache: Vec<((String, String), CompanyTypes)>,
    pub regiments_row: Entity,
    pub battalions_row: Entity,
    pub companies_row: Entity,
    pub platoons_row: Entity,
    pub squads_row: Entity,
    pub toggle_production_button: (Entity, LimitedNumber<0, 2>),
    pub current_regiment: i32,
    pub squad_specialization_dropdown_lists: (i32, Vec<Entity>),
    pub company_type_dropdown_lists: (i32, Vec<Entity>),
}

#[derive(Component)]
pub struct SquadSelector(pub (i32, (CompanyTypes, (i32, i32, i32, i32, i32), bool, Entity)));

#[derive(Component)]
pub struct PlatoonSelector(pub (i32, (CompanyTypes, Vec<(i32, i32, i32, i32, i32)>, bool, Entity)));

#[derive(Component)]
pub struct CompanySelector(pub (i32, (CompanyTypes, Vec<(i32, i32, i32, i32, i32)>, bool, Entity)));

#[derive(Component)]
pub struct BattalionSelector(pub (i32, (Vec<(CompanyTypes, (i32, i32, i32, i32, i32))>, bool, Entity)));

#[derive(Component)]
pub struct RegimentSelector(pub (i32, (Vec<(CompanyTypes, (i32, i32, i32, i32, i32))>, bool, Entity)));

#[derive(Component)]
pub struct BrigadeSelector(pub (i32, (Vec<(CompanyTypes, (i32, i32, i32, i32, i32))>, bool, Entity)));

#[derive(Component)]
pub struct ArtilleryUnitSelector(pub (i32, i32, bool, Entity));

#[derive(Component)]
pub struct ButtonAction {
    pub action: Actions,
}

#[derive(Component)]
pub struct ParentNode;

#[derive(Component)]
pub struct DisplayedModelHolder;

#[derive(Component)]
pub struct ResourceZoneRestricted;

#[derive(Resource)]
pub struct UiButtonNodes {
    pub left_bottom_node: Entity,
    pub left_bottom_node_rows: Vec<Entity>,
    pub is_left_bottom_node_visible: bool,
    pub middle_bottom_node: Entity,
    pub middle_bottom_node_row: Entity,
    pub is_middle_bottom_node_visible: bool,
    pub middle_upper_node: Entity,
    pub middle_upper_node_row: Entity,
    pub right_bottom_node: Entity,
    pub right_bottom_node_rows: Vec<Entity>,
    pub symbol_level_dropdown_list: Entity,
    pub is_middle_upper_node_visible: bool,
    pub hint_node: Entity,
    pub hint_text: Entity,
    pub middle_upper_node_width: f32,
    pub margin: f32,
    pub button_size: f32,
}

#[derive(Resource)]
pub struct Specializations {
    pub regular: Vec<(String, String)>,
    pub shock: Vec<(String, String)>,
    pub armored: Vec<(String, String)>,
}

#[derive(Resource)]
pub struct BuildingPlacementCache {
    pub is_active: bool,
    pub current_building: BuildingsBundles,
    pub current_building_y_adjustment: f32,
    pub current_building_check_collider: Collider,
    pub needed_buildpower: i32,
    pub name: String,
    pub build_distance: f32,
    pub resource_cost: i32,
}

#[derive(Resource)]
pub struct UiBlocker {
    pub is_bottom_left_node_blocked: bool,
    pub is_bottom_middle_node_blocked: bool,
}

pub fn setup_ingame_ui(
    mut commands: Commands,
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    army: Res<Armies>,
    other_assets: Res<OtherAssets>,
    asset_server: Res<AssetServer>,
    player_data: Res<PlayerData>,
){
    let window = windows_q.single();
    let window_width = window.physical_width();
    let window_height = window.physical_height();

    let left_bottom_node_size = window_width / 8;
    let left_bottom_node = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(left_bottom_node_size as f32),
            height: Val::Px(left_bottom_node_size as f32),
            top: Val::Px((window_height - left_bottom_node_size) as f32),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        ..default()
    };

    let left_bottom_node_entity = commands.spawn(left_bottom_node).insert(ParentNode).id();

    ui_button_nodes.left_bottom_node = left_bottom_node_entity;

    commands.entity(left_bottom_node_entity).insert(Visibility::Hidden);

    ui_button_nodes.margin = (left_bottom_node_size / 30) as f32;

    let mut left_bottom_node_rows: Vec<Entity> = Vec::new();
    commands.entity(left_bottom_node_entity).with_children(|parent| {
        for _i in 0..3 {
            left_bottom_node_rows.push(
                parent.spawn(NodeBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px((left_bottom_node_size) as f32),
                        height: Val::Px((left_bottom_node_size / 3 - (ui_button_nodes.margin * 2.) as u32) as f32),
                        flex_direction: FlexDirection::Row,
                        margin: UiRect {
                            left: Val::Px(0.),
                            right: Val::Px(0.),
                            top: Val::Px(ui_button_nodes.margin),
                            bottom: Val::Px(ui_button_nodes.margin),
                        },
                        ..default()
                    },
                    background_color: Color::srgba(0., 0., 0., 0.).into(),
                    ..default()
                }).id()
            );
        }
    });

    ui_button_nodes.left_bottom_node_rows = left_bottom_node_rows.clone();

    let middle_bottom_node_width = window_width - left_bottom_node_size * 2;
    let middle_bottom_node_height = left_bottom_node_size / 3;
    let middle_bottom_node = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(middle_bottom_node_width as f32),
            height: Val::Px(middle_bottom_node_height as f32),
            top: Val::Px((window_height - middle_bottom_node_height) as f32),
            left: Val::Px((window_width - middle_bottom_node_width - left_bottom_node_size) as f32),
            justify_content: JustifyContent::Center,
            ..default()
        },
        background_color: Color::srgba(0., 0., 0., 0.).into(),
        ..default()
    };

    let middle_bottom_node_entity = commands.spawn(middle_bottom_node).insert(ParentNode).id();

    ui_button_nodes.middle_bottom_node = middle_bottom_node_entity;

    commands.entity(middle_bottom_node_entity).insert(Visibility::Hidden);

    let hint_node = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(middle_bottom_node_width as f32 * 0.8),
            height: Val::Px(middle_bottom_node_height as f32 * 4.),
            bottom: Val::Px((middle_bottom_node_height) as f32 * 1.5),
            left: Val::Px((window_width - (middle_bottom_node_width as f32 * 0.9) as u32 - left_bottom_node_size) as f32),
            justify_content: JustifyContent::Start,
            ..default()
        },
        background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        ..default()
    };

    let hint_node_entity = commands.spawn(hint_node).insert(ParentNode).id();
    
    ui_button_nodes.hint_node = hint_node_entity;

    let mut hint_node_sub_bundle_entity = Entity::PLACEHOLDER;

    commands.entity(ui_button_nodes.hint_node).with_children(|parent| {
        hint_node_sub_bundle_entity = parent.spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Relative,
                width: Val::Px(middle_bottom_node_width as f32 * 0.8),
                height: Val::Px(middle_bottom_node_height as f32 * 4.),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Start,
                ..default()
            },
            background_color: Color::srgba(0., 0., 0., 0.).into(),
            ..default()
        }).id();
    });

    ui_button_nodes.hint_text = commands.entity(hint_node_sub_bundle_entity).insert(TextBundle{
        text: Text{
            sections: vec![TextSection {
                value: "".to_string(),
                ..default()
            }],
            justify: JustifyText::Left,
            ..default() 
        },
        ..default()
    }).id();

    commands.entity(ui_button_nodes.hint_node).insert(Visibility::Hidden);

    let right_bottom_node_size = left_bottom_node_size;
    let right_bottom_node = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(right_bottom_node_size as f32),
            height: Val::Px(right_bottom_node_size as f32),
            top: Val::Px((window_height - right_bottom_node_size) as f32),
            left: Val::Px((window_width - right_bottom_node_size) as f32),
            ..default()
        },
        background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        ..default()
    };

    let right_bottom_node_entity = commands.spawn(right_bottom_node).insert(ParentNode).id();

    ui_button_nodes.right_bottom_node = right_bottom_node_entity;

    let mut right_bottom_node_rows: Vec<Entity> = Vec::new();
    commands.entity(right_bottom_node_entity).with_children(|parent| {
        for _i in 0..3 {
            right_bottom_node_rows.push(
                parent.spawn(NodeBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px((left_bottom_node_size) as f32),
                        height: Val::Px((left_bottom_node_size / 3 - (ui_button_nodes.margin * 2.) as u32) as f32),
                        flex_direction: FlexDirection::Row,
                        margin: UiRect {
                            left: Val::Px(0.),
                            right: Val::Px(0.),
                            top: Val::Px(ui_button_nodes.margin),
                            bottom: Val::Px(ui_button_nodes.margin),
                        },
                        ..default()
                    },
                    background_color: Color::srgba(0., 0., 0., 0.).into(),
                    ..default()
                }).id()
            );
        }
    });

    ui_button_nodes.right_bottom_node_rows = right_bottom_node_rows;

    ui_button_nodes.button_size = (left_bottom_node_size / 3) as f32;

    let button_amount = (middle_bottom_node_width as f32 / ui_button_nodes.button_size - 1.) as i32;
    let mut middle_node_row: Entity = Entity::PLACEHOLDER;
    commands.entity(middle_bottom_node_entity).with_children(|parent| {
        middle_node_row = parent.spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Relative,
                width: Val::Px(button_amount as f32 * ui_button_nodes.button_size),
                height: Val::Px(middle_bottom_node_height as f32),
                ..default()
            },
            background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
            ..default()
        }).id()
    });

    ui_button_nodes.middle_bottom_node_row = middle_node_row;



    // let right_center_node_width = left_bottom_node_size / 3;
    // let right_center_node_height = right_center_node_width * 3;

    // let right_center_node = NodeBundle {
    //     style: Style {
    //         position_type: PositionType::Absolute,
    //         width: Val::Px(right_center_node_width as f32),
    //         height: Val::Px(right_center_node_height as f32),
    //         top: Val::Px((window_height - right_bottom_node_size - right_center_node_height - right_center_node_width) as f32),
    //         right: Val::Px(0.),
    //         flex_direction: FlexDirection::Column,
    //         ..default()
    //     },
    //     background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
    //     ..default()
    // };

    // let right_center_node_entity = commands.spawn(right_center_node).insert(ParentNode).id();

    commands.entity(ui_button_nodes.right_bottom_node_rows[0]).with_children(|parent| {
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
            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
            ..default()
        }).insert(ButtonAction{action: Actions::OpenArmySettings})
        .with_children(|button_parent| {
            button_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "L".to_string(),
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default() 
                },
                ..default()
            });
        });
    });

    commands.entity(ui_button_nodes.right_bottom_node_rows[0]).with_children(|parent| {
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
            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
            ..default()
        }).insert(ButtonAction{action: Actions::OpenBuildingsList})
        .with_children(|button_parent| {
            button_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "B".to_string(),
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default() 
                },
                ..default()
            });
        });
    });

    commands.entity(ui_button_nodes.right_bottom_node_rows[0]).with_children(|parent| {
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
            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
            ..default()
        }).insert(ButtonAction{action: Actions::OpenTacticalSymbolsLevels})
        .with_children(|button_parent| {
            ui_button_nodes.symbol_level_dropdown_list = button_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "Squads".to_string(),
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                style: Style {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ..default()
            }).id();
        });
    });

    let middle_upper_node_width = 300.;
    let middle_upper_node = commands.spawn(NodeBundle{
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(middle_upper_node_width),
            height: Val::Px(middle_bottom_node_height as f32),
            top: Val::Px(0.),
            left: Val::Px((window_width as f32 / 2.) - (middle_upper_node_width / 2.)),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            ..default()
        },
        background_color: Color::srgba(0., 0., 0., 0.).into(),
        ..default()
    })
    .insert(ParentNode)
    .insert(Visibility::Hidden)
    .id();

    let mut middle_upper_node_row = Entity::PLACEHOLDER;
    commands.entity(middle_upper_node).with_children(|parent| {
        middle_upper_node_row = parent.spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Relative,
                width: Val::Px(middle_upper_node_width),
                height: Val::Px(middle_bottom_node_height as f32),
                justify_content: JustifyContent::Center,
                ..default()
            },
            background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
            ..default()
        }).id()
    });

    ui_button_nodes.middle_upper_node = middle_upper_node;
    ui_button_nodes.middle_upper_node_row = middle_upper_node_row;
    ui_button_nodes.is_middle_upper_node_visible = false;
    ui_button_nodes.middle_upper_node_width = middle_upper_node_width;

    // commands.entity(middle_upper_node_row).with_children(|parent| {
    //     parent.spawn(ButtonBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(middle_upper_node_width),
    //             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //             margin: UiRect {
    //                 left: Val::Px(ui_button_nodes.margin),
    //                 right: Val::Px(ui_button_nodes.margin),
    //                 top: Val::Px(ui_button_nodes.margin),
    //                 bottom: Val::Px(ui_button_nodes.margin),
    //             },
    //             justify_content: JustifyContent::Center,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //         ..default()
    //     });
    // });

    let land_army_settings_node_width = window_width - (ui_button_nodes.button_size) as u32;
    let land_army_settings_node_height = window_height - left_bottom_node_size - (ui_button_nodes.button_size) as u32;

    army_settings_nodes.land_army_settings_node_width = land_army_settings_node_width;
    army_settings_nodes.land_army_settings_node_height = land_army_settings_node_height;

    let land_army_settings_node = NodeBundle {
        style: Style {
            position_type: PositionType::Absolute,
            width: Val::Px(land_army_settings_node_width as f32),
            height: Val::Px(land_army_settings_node_height as f32),
            top: Val::Px(ui_button_nodes.button_size * 0.5),
            left: Val::Px(ui_button_nodes.button_size / 2.),
            ..default()
        },
        background_color: Color::srgba(0.1, 0.1, 0.1, 0.25).into(),
        ..default()
    };

    let land_army_settings_node_entity = commands.spawn(land_army_settings_node).insert(ParentNode).id();

    commands.entity(land_army_settings_node_entity).insert(Visibility::Hidden);

    army_settings_nodes.land_army_settings_node = land_army_settings_node_entity;

    let mut left_army_node = Entity::PLACEHOLDER;
    let mut middle_army_node = Entity::PLACEHOLDER;
    let mut right_army_node = Entity::PLACEHOLDER;
    let mut toggle_button_node = Entity::PLACEHOLDER;

    commands.entity(land_army_settings_node_entity).with_children(|parent|{
        left_army_node = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.1),
                    height: Val::Px(land_army_settings_node_height as f32),
                    top: Val::Px(0.),
                    left: Val::Px(0.),
                    ..default()
                },
                background_color: Color::srgba(1.1, 0.1, 0.1, 0.).into(),
                ..default()
            }
        ).id();

        middle_army_node = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32),
                    top: Val::Px(0.),
                    left: Val::Px(0.),
                    flex_direction: FlexDirection::Row,
                    ..default()
                },
                background_color: Color::srgba(0.1, 1.1, 0.1, 0.).into(),
                ..default()
            }
        ).id();

        right_army_node = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.1),
                    height: Val::Px(land_army_settings_node_height as f32),
                    top: Val::Px(0.),
                    left: Val::Px(0.),
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 1.1, 0.).into(),
                ..default()
            }
        ).id();
    });

    let swipe_button_height = ui_button_nodes.button_size * 3.;

    let mut left_swipe_button = Entity::PLACEHOLDER;
    let mut right_swipe_button = Entity::PLACEHOLDER;

    commands.entity(left_army_node).with_children(|parent| {
        left_swipe_button = parent.spawn(ButtonBundle{
            style: Style {
                position_type: PositionType::Absolute,
                width: Val::Px(ui_button_nodes.button_size),
                height: Val::Px(swipe_button_height),
                margin: UiRect {
                    left: Val::Px(ui_button_nodes.margin),
                    right: Val::Px(ui_button_nodes.margin),
                    top: Val::Px(ui_button_nodes.margin),
                    bottom: Val::Px(ui_button_nodes.margin),
                },
                top: Val::Px(land_army_settings_node_height as f32 / 2. - swipe_button_height / 2.),
                left: Val::Px(land_army_settings_node_width as f32 * 0.1 / 2. - ui_button_nodes.button_size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
            ..default()
        }).insert(ButtonAction{
            action: Actions::SwipeRegiment(0),
        })
        .with_children(|button_parent| {
            button_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "<".to_string(),
                        style: TextStyle {
                            font_size: 100.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        }).id();
    });

    commands.entity(right_army_node).with_children(|parent| {
        right_swipe_button = parent.spawn(ButtonBundle{
            style: Style {
                position_type: PositionType::Absolute,
                width: Val::Px(ui_button_nodes.button_size),
                height: Val::Px(swipe_button_height),
                margin: UiRect {
                    left: Val::Px(ui_button_nodes.margin),
                    right: Val::Px(ui_button_nodes.margin),
                    top: Val::Px(ui_button_nodes.margin),
                    bottom: Val::Px(ui_button_nodes.margin),
                },
                top: Val::Px(land_army_settings_node_height as f32 / 2. - swipe_button_height / 2.),
                right: Val::Px(land_army_settings_node_width as f32 * 0.1 / 2. - ui_button_nodes.button_size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
            ..default()
        }).insert(ButtonAction{
            action: Actions::SwipeRegiment(1),
        })
        .with_children(|button_parent| {
            button_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: ">".to_string(),
                        style: TextStyle {
                            font_size: 100.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        }).id();
    });

    let mut regiments_row = Entity::PLACEHOLDER;
    let mut battalions_row = Entity::PLACEHOLDER;
    let mut companies_row = Entity::PLACEHOLDER;
    let mut platoons_row = Entity::PLACEHOLDER;
    let mut squads_row = Entity::PLACEHOLDER;

    commands.entity(middle_army_node).with_children(|parent| {
        toggle_button_node = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.2),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32),
                    left: Val::Px(land_army_settings_node_width as f32 * 0.8 /2. - land_army_settings_node_width as f32 * 0.2 / 2.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::ToggleProduction}
        )
        .with_children(|bar_parent| {
            army_settings_nodes.toggle_production_button = (
                bar_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Ready to start".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id(),
                LimitedNumber::new(),
            );
        })
        .id();

        regiments_row = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32 / 6.),
                    top: Val::Px(0.),
                    left: Val::Px(0.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.5, 0.).into(),
                ..default()
            }
        ).id();

        battalions_row = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32 / 6.),
                    top: Val::Px(land_army_settings_node_height as f32 / 6.),
                    left: Val::Px(0.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.5, 0.1, 0.1, 0.).into(),
                ..default()
            }
        ).id();

        companies_row = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32 / 6.),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 2.),
                    left: Val::Px(0.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.5, 0.).into(),
                ..default()
            }
        ).id();

        platoons_row = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32 / 6.),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 3.),
                    left: Val::Px(0.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.5, 0.1, 0.1, 0.).into(),
                ..default()
            }
        ).id();

        squads_row = parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(land_army_settings_node_width as f32 * 0.8),
                    height: Val::Px(land_army_settings_node_height as f32 / 6.),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 4.),
                    left: Val::Px(0.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.5, 0.).into(),
                ..default()
            }
        ).id();
    });

    army_settings_nodes.regiments_row = regiments_row;
    army_settings_nodes.battalions_row = battalions_row;
    army_settings_nodes.companies_row = companies_row;
    army_settings_nodes.platoons_row = platoons_row;
    army_settings_nodes.squads_row = squads_row;

    commands.entity(regiments_row).with_children(|parent| {
        parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(land_army_settings_node_width as f32 * 0.8 / 2. - ui_button_nodes.button_size * 2. / 2.),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "1st Regiment".to_string(),
                        style: TextStyle {
                            font_size: 30.,
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

    commands.entity(battalions_row).with_children(|parent| {
        parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.5
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "1st Battalion".to_string(),
                        style: TextStyle {
                            font_size: 30.,
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

        parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "2nd Battalion".to_string(),
                        style: TextStyle {
                            font_size: 30.,
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

        parent.spawn(
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.1
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.5
                        + ui_button_nodes.button_size * 2. / 2.
                        + ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "3rd Battalion".to_string(),
                        style: TextStyle {
                            font_size: 30.,
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

    let mut company1 = Entity::PLACEHOLDER;
    let mut company2 = Entity::PLACEHOLDER;
    let mut company3 = Entity::PLACEHOLDER;
    let mut company4 = Entity::PLACEHOLDER;
    let mut company5 = Entity::PLACEHOLDER;
    let mut company6 = Entity::PLACEHOLDER;
    let mut company7 = Entity::PLACEHOLDER;
    let mut company8 = Entity::PLACEHOLDER;
    let mut company9 = Entity::PLACEHOLDER;

    let mut company_dropdown1 = Entity::PLACEHOLDER;
    let mut company_dropdown2 = Entity::PLACEHOLDER;
    let mut company_dropdown3 = Entity::PLACEHOLDER;
    let mut company_dropdown4 = Entity::PLACEHOLDER;
    let mut company_dropdown5 = Entity::PLACEHOLDER;
    let mut company_dropdown6 = Entity::PLACEHOLDER;
    let mut company_dropdown7 = Entity::PLACEHOLDER;
    let mut company_dropdown8 = Entity::PLACEHOLDER;
    let mut company_dropdown9 = Entity::PLACEHOLDER;

    commands.entity(companies_row).with_children(|parent| {
        company1 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.5
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((0, (1, 1, 1)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "1st Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((0, (1,1,1)))}
            )
            .with_children(|button_parent| {
                company_dropdown1 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company2 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.5
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((1, (1, 1, 2)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "2nd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((1, (1, 1, 2)))}
            )
            .with_children(|button_parent| {
                company_dropdown2 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company3 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.5
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((2, (1, 1, 3)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "3rd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((2, (1, 1, 3)))}
            )
            .with_children(|button_parent| {
                company_dropdown3 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company4 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                        - ui_button_nodes.button_size * 2.
                        - ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((3, (1, 2, 1)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "1st Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((3, (1, 2, 1)))}
            )
            .with_children(|button_parent| {
                company_dropdown4 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company5 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        - ui_button_nodes.button_size * 2. / 2.
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((4, (1, 2, 2)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "2nd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((4, (1, 2, 2)))}
            )
            .with_children(|button_parent| {
                company_dropdown5 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company6 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        + ui_button_nodes.button_size * 2. / 2.
                        + ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((5, (1, 2, 3)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "3rd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((5, (1, 2, 3)))}
            )
            .with_children(|button_parent| {
                company_dropdown6 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company7 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.1
                        + ui_button_nodes.button_size * 2. / 2.
                        + ui_button_nodes.button_size * 0.5
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((6, (1, 3, 1)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "1st Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((6, (1, 3, 1)))}
            )
            .with_children(|button_parent| {
                company_dropdown7 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company8 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.1
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.5
                        + ui_button_nodes.button_size * 2. / 2.
                        + ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((7, (1, 3, 2)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "2nd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((7, (1, 3, 2)))}
            )
            .with_children(|button_parent| {
                company_dropdown8 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();

        company9 = parent.spawn(
            ButtonBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ui_button_nodes.button_size * 2.),
                    height: Val::Px(ui_button_nodes.button_size),
                    top: Val::Px(land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                    left: Val::Px(
                        land_army_settings_node_width as f32 * 0.8 / 2.
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.1
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.5
                        + ui_button_nodes.button_size * 2.
                        + ui_button_nodes.button_size * 0.1
                        + ui_button_nodes.button_size * 2. / 2.
                        + ui_button_nodes.button_size * 0.1
                    ),
                    align_content: AlignContent::Center,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    justify_items: JustifyItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }
        )
        .insert(
            ButtonAction{action: Actions::SetupCompany((8, (1, 3, 3)))}
        )
        .with_children(|bar_parent| {
            bar_parent.spawn(TextBundle {
                text: Text{
                    sections: vec![TextSection {
                        value: "3rd Company".to_string(),
                        style: TextStyle {
                            font_size: 30.,
                            ..default()
                        },
                        ..default()
                    }],
                    justify: JustifyText::Center,
                    ..default()
                },
                ..default()
            });
        })
        .with_children(|bar_parent| {
            bar_parent.spawn(
                ButtonBundle {
                    style: Style {
                        position_type: PositionType::Absolute,
                        width: Val::Px(ui_button_nodes.button_size * 2.),
                        height: Val::Px(ui_button_nodes.button_size * 0.4),
                        top: Val::Px(ui_button_nodes.button_size * 1.1),
                        left: Val::Px(0.),
                        align_content: AlignContent::Center,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        justify_items: JustifyItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }
            )
            .insert(
                ButtonAction{action: Actions::OpenCompanyTypes((8, (1, 3, 3)))}
            )
            .with_children(|button_parent| {
                company_dropdown9 = button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Regular".to_string(),
                            style: TextStyle {
                                font_size: 30.,
                                ..default()
                            },
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default()
                    },
                    ..default()
                }).id();
            });
        })
        .id();
    });

    army_settings_nodes.company_buttons.2 = vec![
        company1,
        company2,
        company3,
        company4,
        company5,
        company6,
        company7,
        company8,
        company9,
    ];

    army_settings_nodes.company_type_dropdown_lists.1 = vec![
        company_dropdown1,
        company_dropdown2,
        company_dropdown3,
        company_dropdown4,
        company_dropdown5,
        company_dropdown6,
        company_dropdown7,
        company_dropdown8,
        company_dropdown9,
    ];

    // let mut divisions_row = Entity::PLACEHOLDER;
    // let mut brigades_row = Entity::PLACEHOLDER;
    // let mut batallions_row = Entity::PLACEHOLDER;
    // let mut companies_row = Entity::PLACEHOLDER;
    // let mut platoons_row = Entity::PLACEHOLDER;
    // let mut units_row = Entity::PLACEHOLDER;

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     divisions_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px(land_army_settings_node_height as f32 / 6.5),
    //             flex_direction: FlexDirection::Row,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     brigades_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px(land_army_settings_node_height as f32 / 6.5),
    //             flex_direction: FlexDirection::Row,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     batallions_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px((land_army_settings_node_height / 5) as f32),
    //             flex_direction: FlexDirection::Row,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     companies_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px(land_army_settings_node_height as f32 / 6.5),
    //             flex_direction: FlexDirection::Row,
    //             justify_content: JustifyContent::Center,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     platoons_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px((land_army_settings_node_height / 5) as f32),
    //             flex_direction: FlexDirection::Row,
    //             justify_content: JustifyContent::Center,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // commands.entity(land_army_settings_node_entity).with_children(|parent| {
    //     units_row = parent.spawn(NodeBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(land_army_settings_node_width as f32),
    //             height: Val::Px(land_army_settings_node_height as f32 / 6.5),
    //             flex_direction: FlexDirection::Row,
    //             justify_content: JustifyContent::End,
    //             align_items: AlignItems::End,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 0.).into(),
    //         ..default()
    //     }).id();
    // });

    // army_settings_nodes.companies_row = companies_row;
    // army_settings_nodes.platoons_row = platoons_row;
    // army_settings_nodes.units_row = units_row;

    // let mut division_placeholders: Vec<Entity> = Vec::new();
    // for _i in 0..2 {
    //     commands.entity(divisions_row).with_children(|parent| {
    //         division_placeholders.push(
    //             parent.spawn(NodeBundle{
    //                 style: Style {
    //                     position_type: PositionType::Relative,
    //                     width: Val::Px((land_army_settings_node_width / 2) as f32 - ui_button_nodes.margin * 2.),
    //                     height: Val::Px(land_army_settings_node_height as f32 / 6.5 - ui_button_nodes.margin * 2.),
    //                     margin: UiRect {
    //                         left: Val::Px(ui_button_nodes.margin),
    //                         right: Val::Px(ui_button_nodes.margin),
    //                         top: Val::Px(ui_button_nodes.margin),
    //                         bottom: Val::Px(ui_button_nodes.margin),
    //                     },
    //                     flex_direction: FlexDirection::Column,
    //                     justify_content: JustifyContent::Center,
    //                     align_items: AlignItems::Center,
    //                     ..default()
    //                 },
    //                 background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
    //                 ..default()
    //             }).id()
    //         );
    //     });
    // }

    // let mut brigade_placeholders: Vec<Entity> = Vec::new();
    // for _i in 0..6 {
    //     commands.entity(brigades_row).with_children(|parent| {
    //         brigade_placeholders.push(
    //             parent.spawn(NodeBundle{
    //                 style: Style {
    //                     position_type: PositionType::Relative,
    //                     width: Val::Px((land_army_settings_node_width / 6) as f32 - ui_button_nodes.margin * 2.),
    //                     height: Val::Px(land_army_settings_node_height as f32 / 6.5 - ui_button_nodes.margin * 2.),
    //                     margin: UiRect {
    //                         left: Val::Px(ui_button_nodes.margin),
    //                         right: Val::Px(ui_button_nodes.margin),
    //                         top: Val::Px(ui_button_nodes.margin),
    //                         bottom: Val::Px(ui_button_nodes.margin),
    //                     },
    //                     flex_direction: FlexDirection::Column,
    //                     justify_content: JustifyContent::Center,
    //                     align_items: AlignItems::Center,
    //                     ..default()
    //                 },
    //                 background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
    //                 ..default()
    //             }).id()
    //         );
    //     });
    // }

    // let mut batallion_placeholders: Vec<Entity> = Vec::new();
    // for _i in 0..18 {
    //     commands.entity(batallions_row).with_children(|parent| {
    //         batallion_placeholders.push(
    //             parent.spawn(NodeBundle{
    //                 style: Style {
    //                     position_type: PositionType::Relative,
    //                     width: Val::Px((land_army_settings_node_width / 18) as f32 - ui_button_nodes.margin * 2.),
    //                     height: Val::Px((land_army_settings_node_height / 5) as f32 - ui_button_nodes.margin * 2.),
    //                     margin: UiRect {
    //                         left: Val::Px(ui_button_nodes.margin),
    //                         right: Val::Px(ui_button_nodes.margin),
    //                         top: Val::Px(ui_button_nodes.margin),
    //                         bottom: Val::Px(ui_button_nodes.margin),
    //                     },
    //                     flex_direction: FlexDirection::Column,
    //                     justify_content: JustifyContent::Center,
    //                     align_items: AlignItems::Center,
    //                     ..default()
    //                 },
    //                 background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
    //                 ..default()
    //             }).id()
    //         );
    //     });
    // }

    // for placeholder in division_placeholders {
    //     commands.entity(placeholder).with_children(|parent| {
    //         parent.spawn(ButtonBundle{
    //             style: Style {
    //                 position_type: PositionType::Relative,
    //                 width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                 height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                 margin: UiRect {
    //                     left: Val::Px(ui_button_nodes.margin),
    //                     right: Val::Px(ui_button_nodes.margin),
    //                     top: Val::Px(ui_button_nodes.margin),
    //                     bottom: Val::Px(ui_button_nodes.margin),
    //                 },
    //                 justify_content: JustifyContent::Center,
    //                 align_items: AlignItems::Center,
    //                 ..default()
    //             },
    //             background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //             ..default()
    //         })
    //         .with_children(|button_parent| {
    //             button_parent.spawn(TextBundle {
    //                 text: Text{
    //                     sections: vec![TextSection {
    //                         value: "Division".to_string(),
    //                         style: TextStyle {
    //                             font_size: 20.,
    //                             ..default()
    //                         },
    //                         ..default()
    //                     }],
    //                     justify: JustifyText::Center,
    //                     ..default() 
    //                 },
    //                 ..default()
    //             });
    //         });
    //     });
    // }

    // for placeholder in brigade_placeholders {
    //     commands.entity(placeholder).with_children(|parent| {
    //         parent.spawn(ButtonBundle{
    //             style: Style {
    //                 position_type: PositionType::Relative,
    //                 width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                 height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                 margin: UiRect {
    //                     left: Val::Px(ui_button_nodes.margin),
    //                     right: Val::Px(ui_button_nodes.margin),
    //                     top: Val::Px(ui_button_nodes.margin),
    //                     bottom: Val::Px(ui_button_nodes.margin),
    //                 },
    //                 justify_content: JustifyContent::Center,
    //                 align_items: AlignItems::Center,
    //                 ..default()
    //             },
    //             background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //             ..default()
    //         })
    //         .with_children(|button_parent| {
    //             button_parent.spawn(TextBundle {
    //                 text: Text{
    //                     sections: vec![TextSection {
    //                         value: "Brigade".to_string(),
    //                         style: TextStyle {
    //                             font_size: 20.,
    //                             ..default()
    //                         },
    //                         ..default()
    //                     }],
    //                     justify: JustifyText::Center,
    //                     ..default() 
    //                 },
    //                 ..default()
    //             });
    //         });
    //     });
    // }

    // let mut division_number: LimitedNumber<1, 3> = LimitedNumber::new();
    // let mut brigade_number: LimitedNumber<1, 3> = LimitedNumber::new();
    // let mut batallion_number: LimitedNumber<1, 3> = LimitedNumber::new();
    // batallion_number.set_value(0);
    // let mut counter = -1;
    
    // let regular_count = START_REGULAR_SQUADS_AMOUNT / 9;
    // let shock_count = START_SHOCK_SQUADS_AMOUNT / 9;
    // let armored_count = START_ARMORED_SQUADS_AMOUNT / 9;
    // let mut battalion_type = CompanyTypes::None;
    // let mut battalion_type_name = "".to_string();

    // for placeholder in batallion_placeholders {
    //     if batallion_number.next() {
    //         if brigade_number.next() {
    //             division_number.next();
    //         }
    //     }

    //     counter += 1;
        
    //     commands.entity(placeholder).with_children(|parent| {
    //         army_settings_nodes.batallion_buttons.push((
    //             parent.spawn(ButtonBundle{
    //                 style: Style {
    //                     position_type: PositionType::Relative,
    //                     width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                     height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                     margin: UiRect {
    //                         left: Val::Px(ui_button_nodes.margin),
    //                         right: Val::Px(ui_button_nodes.margin),
    //                         top: Val::Px(ui_button_nodes.margin),
    //                         bottom: Val::Px(ui_button_nodes.margin),
    //                     },
    //                     justify_content: JustifyContent::Center,
    //                     align_items: AlignItems::Center,
    //                     ..default()
    //                 },
    //                 background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //                 ..default()
    //             }).insert(ButtonAction{
    //                 action: Actions::SetupBatallion((counter, (division_number.get_value(), brigade_number.get_value(), batallion_number.get_value())))}
    //             )
    //             .with_children(|button_parent| {
    //                 button_parent.spawn(TextBundle {
    //                     text: Text{
    //                         sections: vec![TextSection {
    //                             value: "Batallion".to_string(),
    //                             style: TextStyle {
    //                                 font_size: 10.,
    //                                 ..default()
    //                             },
    //                             ..default()
    //                         }],
    //                         justify: JustifyText::Center,
    //                         ..default()
    
    //                     },
    //                     ..default()
    //                 });
    //             }).id(),
    //             LimitedNumber::new()
    //         ));
    //     });

    //     if counter < regular_count {
    //         battalion_type = CompanyTypes::Regular;
    //         battalion_type_name = "Regular".to_string();
    //     } else if counter < regular_count + shock_count {
    //         battalion_type = CompanyTypes::Shock;
    //         battalion_type_name = "Shock".to_string();
    //     } else if counter < regular_count + shock_count + armored_count {
    //         battalion_type = CompanyTypes::Armored;
    //         battalion_type_name = "Armored".to_string();
    //     }

    //     commands.entity(placeholder).with_children(|parent| {
    //         parent.spawn(ButtonBundle{
    //             style: Style {
    //                 position_type: PositionType::Relative,
    //                 width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //                 height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
    //                 margin: UiRect {
    //                     left: Val::Px(ui_button_nodes.margin),
    //                     right: Val::Px(ui_button_nodes.margin),
    //                     top: Val::Px(ui_button_nodes.margin),
    //                     bottom: Val::Px(ui_button_nodes.margin),
    //                 },
    //                 justify_content: JustifyContent::Center,
    //                 align_items: AlignItems::Center,
    //                 ..default()
    //             },
    //             background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //             ..default()
    //         }).insert(ButtonAction{
    //             action: Actions::OpenBatallionTypes((counter, (division_number.get_value(), brigade_number.get_value(), batallion_number.get_value())))}
    //         )
    //         .with_children(|button_parent| {
    //             army_settings_nodes.batallion_type_dropdown_lists.push((
    //                 button_parent.spawn(TextBundle {
    //                     text: Text{
    //                         sections: vec![TextSection {
    //                             value: battalion_type_name.clone(),
    //                             style: TextStyle {
    //                                 font_size: 10.,
    //                                 ..default()
    //                             },
    //                             ..default()
    //                         }],
    //                         justify: JustifyText::Center,
    //                         ..default()
    
    //                     },
    //                     style: Style {
    //                         justify_content: JustifyContent::Center,
    //                         align_items: AlignItems::Center,
    //                         ..default()
    //                     },
    //                     ..default()
    //                 }).id(),
    //                 battalion_type,
    //                 LimitedNumber::new()
    //             ));
    //         });
    //     });
    // }

    // for _i in 0..START_REGULAR_SQUADS_AMOUNT {
    //     army_settings_nodes.platoon_specialization_cache.push((("atgm".to_string(), "ATGM".to_string()), CompanyTypes::Regular));
    // }

    // for _i in START_REGULAR_SQUADS_AMOUNT..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT {
    //     army_settings_nodes.platoon_specialization_cache.push((("lat".to_string(), "LAT".to_string()), CompanyTypes::Shock));
    // }

    // for _i in START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
    //     army_settings_nodes.platoon_specialization_cache.push((("tank".to_string(), "Tank".to_string()), CompanyTypes::Armored));
    // }
    
    // commands.entity(units_row).with_children(|parent| {
    //     parent.spawn(ButtonBundle{
    //         style: Style {
    //             position_type: PositionType::Relative,
    //             width: Val::Px(ui_button_nodes.button_size * 3. - ui_button_nodes.margin * 2.),
    //             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
    //             margin: UiRect {
    //                 left: Val::Px(ui_button_nodes.margin),
    //                 right: Val::Px(ui_button_nodes.margin),
    //                 top: Val::Px(ui_button_nodes.margin),
    //                 bottom: Val::Px(ui_button_nodes.margin),
    //             },
    //             justify_content: JustifyContent::Center,
    //             align_items: AlignItems::Center,
    //             ..default()
    //         },
    //         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
    //         ..default()
    //     }).insert(ButtonAction{action: Actions::ToggleProduction})
    //     .with_children(|button_parent| {
    //         army_settings_nodes.toggle_production_button = (
    //             button_parent.spawn(TextBundle {
    //                 text: Text{
    //                     sections: vec![TextSection {
    //                         value: "Ready to start".to_string(),
    //                         style: TextStyle {
    //                             font_size: 15.,
    //                             ..default()
    //                         },
    //                         ..default()
    //                     }],
    //                     justify: JustifyText::Center,
    //                     ..default()
    //                 },
    //                 ..default()
    //             }).id(),
    //             LimitedNumber::new(),
    //         );
    //     });
    // });

    let mut squad_index: LimitedNumber<1, 3>;
    let mut platoon_index: LimitedNumber<1, 3>;
    let mut company_index: LimitedNumber<1, 3>;
    let mut battalion_index: LimitedNumber<1, 3>;
    let mut regiment_index: LimitedNumber<1, 3>;

    let unit_button_size = ui_button_nodes.button_size * 0.75;

    for i in 1..=2 {
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            let mut bar_entity = Entity::PLACEHOLDER;
            
            let button_entity = commands.spawn(ButtonBundle {
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size),
                    height: Val::Px(unit_button_size),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                ..default()
            })
            .with_children(|parent| {
                    parent.spawn(NodeBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(unit_button_size),
                            height: Val::Px(unit_button_size / 4.),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Start,
                            align_items: AlignItems::Start,
                            top: Val::Px(unit_button_size / 2. + unit_button_size / 4. / 2.),
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).with_children(|parent| {
                        bar_entity = parent.spawn(NodeBundle {
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(unit_button_size),
                                height: Val::Px(unit_button_size / 4.),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Start,
                                align_items: AlignItems::Start,
                                ..default()
                            },
                            background_color: SUPPLIES_COLOR.into(),
                            ..default()
                        })
                        .insert(SuppliesBar {
                            original_width: unit_button_size,
                        }).id();
                    });
                }
            )
            .insert(SquadSelector((
                i,
                (
                    CompanyTypes::None,
                    (
                        regiment_index.get_value(),
                        battalion_index.get_value(),
                        company_index.get_value(),
                        platoon_index.get_value(),
                        squad_index.get_value(),
                    ),
                    false,
                    Entity::PLACEHOLDER,
                ),
            )))
            .insert(ButtonAction{action: Actions::SquadSelection((
                i,
                (
                    CompanyTypes::None,
                    (
                        regiment_index.get_value(),
                        battalion_index.get_value(),
                        company_index.get_value(),
                        platoon_index.get_value(),
                        squad_index.get_value(),
                    ),
                ),
            ))})
            .insert(Visibility::Hidden)
            .id();

            commands.entity(button_entity).insert(SuppliesBarHolder{
                entity: bar_entity,
            });
        }
    }

    let mut current_squad: Vec<(i32, i32, i32, i32, i32)> = Vec::new();

    for i in 1..=2 {
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            current_squad.push((
                regiment_index.get_value(),
                battalion_index.get_value(),
                company_index.get_value(),
                platoon_index.get_value(),
                squad_index.get_value(),
            ));

            if current_squad.len() == PLATOON_SIZE {
                commands.spawn(ButtonBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size),
                        height: Val::Px(unit_button_size),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .insert(PlatoonSelector((
                    i,
                    (
                        CompanyTypes::None,
                        current_squad.clone(),
                        false,
                        Entity::PLACEHOLDER,
                    ),
                )))
                .insert(ButtonAction{action: Actions::PlatoonSelection((
                    i,
                    (
                        CompanyTypes::None,
                        current_squad.clone(),
                    ),
                ))})
                .insert(Visibility::Hidden);

                current_squad.clear();
            }
        }
    }

    let mut current_company: Vec<(i32, i32, i32, i32, i32)> = Vec::new();

    for i in 1..=2 { 
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            current_company.push((
                regiment_index.get_value(),
                battalion_index.get_value(),
                company_index.get_value(),
                platoon_index.get_value(),
                squad_index.get_value(),
            ));

            if current_company.len() == PLATOON_SIZE * COMPANY_SIZE {
                commands.spawn(ButtonBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size),
                        height: Val::Px(unit_button_size),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .insert(CompanySelector((
                    i,
                    (
                    CompanyTypes::None,
                    current_company.clone(),
                    false,
                    Entity::PLACEHOLDER,
                    ),
                )))
                .insert(ButtonAction{action: Actions::CompanySelection((
                    i,
                    (
                    CompanyTypes::None,
                    current_company.clone(),
                    ),
                ))})
                .insert(Visibility::Hidden);

                current_company.clear();
            }
        }
    }

    let mut current_battalion: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

    for i in 1..=2 {
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            current_battalion.push((
                CompanyTypes::None,
                (
                    regiment_index.get_value(),
                    battalion_index.get_value(),
                    company_index.get_value(),
                    platoon_index.get_value(),
                    squad_index.get_value(),
                ),
            ));

            if current_battalion.len() == PLATOON_SIZE * COMPANY_SIZE * BATTALION_SIZE {
                commands.spawn(ButtonBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size),
                        height: Val::Px(unit_button_size),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    image: UiImage::new(other_assets.battalion_symbol_blufor.clone()),
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .insert(BattalionSelector((
                    i,
                    (
                        current_battalion.clone(),
                        false,
                        Entity::PLACEHOLDER,
                    ),
                )))
                .insert(ButtonAction{action: Actions::BattalionSelection(
                    (
                        i,
                        current_battalion.clone(),
                    )
                )})
                .insert(Visibility::Hidden);

                current_battalion.clear();
            }
        }
    }

    let mut current_regiment: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

    for i in 1..=2 {
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            current_regiment.push((
                CompanyTypes::None,
                (
                    regiment_index.get_value(),
                    battalion_index.get_value(),
                    company_index.get_value(),
                    platoon_index.get_value(),
                    squad_index.get_value(),
                ),
            ));

            if current_regiment.len() == PLATOON_SIZE * COMPANY_SIZE * BATTALION_SIZE * REGIMENT_SIZE {
                commands.spawn(ButtonBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size),
                        height: Val::Px(unit_button_size),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    image: UiImage::new(other_assets.regiment_symbol_blufor.clone()),
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .insert(RegimentSelector((
                    i,
                    (
                        current_regiment.clone(),
                        false,
                        Entity::PLACEHOLDER,
                    )
                )))
                .insert(ButtonAction{action: Actions::RegimentSelection(
                    (
                        i,
                        current_regiment.clone(),
                    )
                )})
                .insert(Visibility::Hidden);

                current_regiment.clear();
            }
        }
    }

    let mut current_brigade: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

    for i in 1..=2 {
        squad_index = LimitedNumber::new();
        platoon_index = LimitedNumber::new();
        company_index = LimitedNumber::new();
        battalion_index = LimitedNumber::new();
        regiment_index = LimitedNumber::new();
        squad_index.set_value(0);

        for _i in 0..START_REGULAR_SQUADS_AMOUNT + START_SHOCK_SQUADS_AMOUNT + START_ARMORED_SQUADS_AMOUNT {
            if squad_index.next() {
                if platoon_index.next() {
                    if company_index.next() {
                        if battalion_index.next() {
                            regiment_index.next();
                        }
                    }
                }
            }

            current_brigade.push((
                CompanyTypes::None,
                (
                    regiment_index.get_value(),
                    battalion_index.get_value(),
                    company_index.get_value(),
                    platoon_index.get_value(),
                    squad_index.get_value(),
                ),
            ));

            if current_brigade.len() == PLATOON_SIZE * COMPANY_SIZE * BATTALION_SIZE * REGIMENT_SIZE * ARMY_SIZE {
                commands.spawn(ButtonBundle {
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size),
                        height: Val::Px(unit_button_size),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    image: UiImage::new(other_assets.brigade_symbol_blufor.clone()),
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .insert(BrigadeSelector((
                    i,
                    (
                        current_brigade.clone(),
                        false,
                        Entity::PLACEHOLDER,
                    ),
                )))
                .insert(ButtonAction{action: Actions::BrigadeSelection(
                    (
                        i,
                        current_brigade.clone(),
                    )
                )})
                .insert(Visibility::Hidden);

                current_brigade.clear();
            }
        }
    }

    for team in 1..=2 {
        for number in 1..=START_ARTILLERY_UNITS_COUNT {
            let mut bar_entity = Entity::PLACEHOLDER;
            
            let button_entity = commands.spawn(ButtonBundle {
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size),
                    height: Val::Px(unit_button_size),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                image: UiImage::new(other_assets.artillery_unit_symbol_blufor.clone()),
                background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                ..default()
            })
            .with_children(|parent| {
                    parent.spawn(NodeBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(unit_button_size),
                            height: Val::Px(unit_button_size / 4.),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Start,
                            align_items: AlignItems::Start,
                            top: Val::Px(unit_button_size / 2. + unit_button_size / 4. / 2.),
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).with_children(|parent| {
                        bar_entity = parent.spawn(NodeBundle {
                            style: Style {
                                position_type: PositionType::Relative,
                                width: Val::Px(unit_button_size),
                                height: Val::Px(unit_button_size / 4.),
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Start,
                                align_items: AlignItems::Start,
                                ..default()
                            },
                            background_color: SUPPLIES_COLOR.into(),
                            ..default()
                        })
                        .insert(SuppliesBar {
                            original_width: unit_button_size,
                        }).id();
                    });
                }
            )
            .insert(ArtilleryUnitSelector((
                team,
                number,
                false,
                Entity::PLACEHOLDER,
            )))
            .insert(ButtonAction{action: Actions::ArtilleryUnitSelection((
                team,
                number,
            ))})
            .insert(Visibility::Hidden)
            .id();

            commands.entity(button_entity).insert(SuppliesBarHolder{
                entity: bar_entity,
            });
        }
    }

    commands.spawn(NodeBundle{
        style: Style {
            position_type: PositionType::Relative,
            width: Val::Px(unit_button_size * 10.),
            height: Val::Px(unit_button_size / 2.),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Start,
            ..default()
        },
        background_color: Color::srgba(0., 0., 0., 0.).into(),
        ..default()
    }).with_children(|parent| {
        parent.spawn(NodeBundle{
            style: Style {
                position_type: PositionType::Relative,
                width: Val::Px(unit_button_size * 5.),
                height: Val::Px(unit_button_size / 2.),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Start,
                align_items: AlignItems::Start,
                ..default()
            },
            background_color: Color::srgba(0., 0., 0., 0.).into(),
            ..default()
        }).with_children(|parent| {
            parent.spawn(NodeBundle{
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size / 2.),
                    height: Val::Px(unit_button_size / 2.),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Start,
                    align_items: AlignItems::Start,
                    ..default()
                },
                background_color: MATERIALS_COLOR.into(),
                ..default()
            }).with_children(|parent| {
                parent.spawn(ImageBundle{
                    image: UiImage {
                        texture: other_assets.materials_icon.clone(),
                        ..default()
                    },
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size / 2.),
                        height: Val::Px(unit_button_size / 2.),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::Start,
                        align_items: AlignItems::Start,
                        ..default()
                    },
                    ..default()
                });
            });

            parent.spawn(NodeBundle{
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size * 4.5),
                    height: Val::Px(unit_button_size / 2.),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                ..default()
            }).with_children(|parent| {
                parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "0".to_string(),
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
                }).insert(MaterialsOverallAmountDisplay);
            });
        });

        parent.spawn(NodeBundle{
            style: Style {
                position_type: PositionType::Relative,
                width: Val::Px(unit_button_size * 5.),
                height: Val::Px(unit_button_size / 2.),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Start,
                align_items: AlignItems::Start,
                ..default()
            },
            background_color: Color::srgba(0., 0., 0., 0.).into(),
            ..default()
        }).with_children(|parent| {
            parent.spawn(NodeBundle{
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size / 2.),
                    height: Val::Px(unit_button_size / 2.),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Start,
                    align_items: AlignItems::Start,
                    ..default()
                },
                background_color: HUMAN_RESOURCE_COLOR.into(),
                ..default()
            }).with_children(|parent| {
                parent.spawn(ImageBundle{
                    image: UiImage {
                        texture: other_assets.human_resource_icon.clone(),
                        ..default()
                    },
                    style: Style {
                        position_type: PositionType::Relative,
                        width: Val::Px(unit_button_size / 2.),
                        height: Val::Px(unit_button_size / 2.),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::Start,
                        align_items: AlignItems::Start,
                        ..default()
                    },
                    ..default()
                });
            });

            parent.spawn(NodeBundle{
                style: Style {
                    position_type: PositionType::Relative,
                    width: Val::Px(unit_button_size * 4.5),
                    height: Val::Px(unit_button_size / 2.),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                ..default()
            }).with_children(|parent| {
                parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "0".to_string(),
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
                }).insert(HumanResourcesOverallAmountDisplay);
            });
        });
    });
}

pub fn handle_button_clicks(
    button_interactions_q: Query<(&Interaction, &ButtonAction), (Changed<Interaction>, With<Button>)>,
    other_interactions_q: Query<&Interaction, (Changed<Interaction>, Without<Button>)>,
    mut selection_bounds: ResMut<SelectionBounds>,
    mut event_writer1:
    (
        EventWriter<LandArmyButtonClickEvent>,
        EventWriter<OpenCompanyTypesEvent>,
        EventWriter<ChooseCompanyTypeEvent>,
        EventWriter<SetupCompanyEvent>,
        EventWriter<OpenSquadSpecializationsEvent>,
        EventWriter<ChooseSquadSpecializationEvent>,
        EventWriter<ToggleProductionEvent>,
        EventWriter<SquadSelectionEvent>,
        EventWriter<OpenBuildingsListEvent>,
        EventWriter<BuildingToBuildSelectedEvent>,
        EventWriter<ToggleArtilleryDesignation>,
        EventWriter<CancelArtilleryTargets>,
        EventWriter<CompleteConstruction>,
        EventWriter<OpenTacticalSymbolsLevels>,
        EventWriter<ChangeTacticalSymbolsLevel>,
        EventWriter<PlatoonSelectionEvent>,
    ),
    mut event_writer2:
    (
        EventWriter<CompanySelectionEvent>,
        EventWriter<BattalionSelectionEvent>,
        EventWriter<RegimentSelectionEvent>,
        EventWriter<BrigadeSelectionEvent>,
        EventWriter<ActivateBlueprintsDeletionMode>,
        EventWriter<ActivateBuildingsDeletionMode>,
        EventWriter<ActivateBuildingsDeletionCancelationMode>,
        EventWriter<SwitchBuildingState>,
        EventWriter<RebuildApartments>,
        EventWriter<TransportDisembarkEvent>,
        EventWriter<ArtilleryUnitSelectedEvent>,
        EventWriter<RegimentSwipeEvent>,
    ),
    mut buttons_hover_event_writer: (
        EventWriter<BuildingButtonHovered>,
    ),
    ui_button_nodes: Res<UiButtonNodes>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
){
    if button_interactions_q.is_empty() && other_interactions_q.is_empty() {
        return;
    }

    let mut is_hovered = false;
    
    for (interaction, button_action) in &button_interactions_q {
        match *interaction {
            Interaction::Pressed => {
                match &button_action.action {
                    Actions::OpenArmySettings => {
                        event_writer1.0.send(LandArmyButtonClickEvent);
                    },
                    Actions::OpenCompanyTypes(d) => {
                        event_writer1.1.send(OpenCompanyTypesEvent(*d));
                    },
                    Actions::ChooseCompanyType(d) => {
                        event_writer1.2.send(ChooseCompanyTypeEvent(*d));
                    },
                    Actions::SetupCompany(d) => {
                        event_writer1.3.send(SetupCompanyEvent(*d));
                    },
                    Actions::OpenSquadSpecializations(d) => {
                        event_writer1.4.send(OpenSquadSpecializationsEvent(*d));
                    },
                    Actions::ChooseSquadSpecialization(d) => {
                        event_writer1.5.send(ChooseSquadSpecializationEvent(d.clone()));
                    },
                    Actions::ToggleProduction => {
                        event_writer1.6.send(ToggleProductionEvent);
                    },
                    Actions::SquadSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer1.7.send(SquadSelectionEvent(d.1));
                    },
                    Actions::OpenBuildingsList => {
                        event_writer1.8.send(OpenBuildingsListEvent);
                    },
                    Actions::BuildingToBuildSelected(d) => {
                        event_writer1.9.send(BuildingToBuildSelectedEvent(d.clone()));
                    },
                    Actions::ToggleArtilleryDesignation => {
                        event_writer1.10.send(ToggleArtilleryDesignation);
                    },
                    Actions::CancelArtilleryTargets => {
                        event_writer1.11.send(CancelArtilleryTargets);
                    },
                    Actions::CompleteConstruction => {
                        event_writer1.12.send(CompleteConstruction);
                    },
                    Actions::OpenTacticalSymbolsLevels => {
                        event_writer1.13.send(OpenTacticalSymbolsLevels);
                    },
                    Actions::ChangeTacticalSymbolsLevel(d) => {
                        event_writer1.14.send(ChangeTacticalSymbolsLevel(d.clone()));
                    },
                    Actions::PlatoonSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer1.15.send(PlatoonSelectionEvent(d.1.clone()));
                    },
                    Actions::CompanySelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer2.0.send(CompanySelectionEvent(d.1.clone()));
                    },
                    Actions::BattalionSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer2.1.send(BattalionSelectionEvent(d.1.clone()));
                    },
                    Actions::RegimentSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer2.2.send(RegimentSelectionEvent(d.1.clone()));
                    },
                    Actions::BrigadeSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer2.3.send(BrigadeSelectionEvent(d.1.clone()));
                    },
                    Actions::ActivateBlueprintsDeletionMode => {
                        event_writer2.4.send(ActivateBlueprintsDeletionMode);
                    },
                    Actions::ActivateBuildingsDeletionMode => {
                        event_writer2.5.send(ActivateBuildingsDeletionMode);
                    },
                    Actions::ActivateBuildingsDeletionCancelationMode => {
                        event_writer2.6.send(ActivateBuildingsDeletionCancelationMode);
                    },
                    Actions::SwitchBuildingState(d) => {
                        event_writer2.7.send(SwitchBuildingState(*d));
                    },
                    Actions::RebuildApartments(d) => {
                        event_writer2.8.send(RebuildApartments(*d));
                    },
                    Actions::DisembarkInfantry => {
                        event_writer2.9.send(TransportDisembarkEvent);
                    },
                    Actions::ArtilleryUnitSelection(d) => {
                        if d.0 != player_data.team {
                            continue;
                        }
                        event_writer2.10.send(ArtilleryUnitSelectedEvent(*d));
                    },
                    Actions::SwipeRegiment(d) => {
                        event_writer2.11.send(RegimentSwipeEvent(*d));
                    },
                    _ => {},
                }

                is_hovered = true;
                selection_bounds.is_ui_hovered = true;
            }
            Interaction::Hovered => {
                is_hovered = true;
                selection_bounds.is_ui_hovered = true;

                match &button_action.action {
                    Actions::BuildingToBuildSelected(d) => {
                        buttons_hover_event_writer.0.send(BuildingButtonHovered(d.4.clone()));
                    }
                    _ => {}
                }
            }
            Interaction::None => {}
        }
    }

    if !is_hovered {
        commands.entity(ui_button_nodes.hint_node).insert(Visibility::Hidden);
    }

    for interaction in other_interactions_q.iter() {
        match *interaction {
            Interaction::Hovered => {
                is_hovered = true;
                selection_bounds.is_ui_hovered = true;
            }
            _ => {}
        }
    }

    if !is_hovered {
        selection_bounds.is_ui_hovered = false;
    }
}



pub fn land_army_settings_system(
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    mut commands: Commands,
    mut event_reader: EventReader<LandArmyButtonClickEvent>,
    game_stage: Res<GameStage>,
){
    for _event in event_reader.read() {
        if matches!(game_stage.0, GameStages::GameStarted) {
            if army_settings_nodes.is_land_army_settings_visible {
                commands.entity(army_settings_nodes.land_army_settings_node).insert(Visibility::Hidden);
                army_settings_nodes.is_land_army_settings_visible = false;
            } else {
                commands.entity(army_settings_nodes.land_army_settings_node).insert(Visibility::Visible);
                army_settings_nodes.is_land_army_settings_visible = true;
            }
        }
    }
}

pub fn regiment_swipe_system(
    mut event_reader: EventReader<RegimentSwipeEvent>,
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    ui_button_nodes: Res<UiButtonNodes>,
    player_data: Res<PlayerData>,
    armies: Res<Armies>,
){
    for event in event_reader.read() {
        let mut current_regiment = army_settings_nodes.current_regiment;
        if event.0 == 0 {
            current_regiment -= 1;

            if current_regiment < 1 {
                current_regiment = ARMY_SIZE as i32;
            }
        } else {
            current_regiment += 1;

            if current_regiment > ARMY_SIZE as i32 {
                current_regiment = 1;
            }
        }

        army_settings_nodes.current_regiment = current_regiment;

        let mut prefix = "".to_string();

        match current_regiment {
            1 => {
                prefix = "1st".to_string();
            },
            2 => {
                prefix = "2nd".to_string();
            },
            3 => {
                prefix = "3rd".to_string();
            },
            _ => {
                prefix = current_regiment.to_string();
            }
        }
        
        commands.entity(army_settings_nodes.regiments_row).despawn_descendants();
        commands.entity(army_settings_nodes.battalions_row).despawn_descendants();
        commands.entity(army_settings_nodes.companies_row).despawn_descendants();
        commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
        commands.entity(army_settings_nodes.squads_row).despawn_descendants();

        army_settings_nodes.company_buttons.0 = -1;
        army_settings_nodes.company_type_dropdown_lists.0 = -1;
        army_settings_nodes.squad_specialization_dropdown_lists.0 = -1;

        if let Some(team_army) = armies.0.get(&player_data.team) {
            commands.entity(army_settings_nodes.regiments_row).with_children(|parent| {
                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2. - ui_button_nodes.button_size * 2. / 2.),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: prefix + " Regiment",
                                style: TextStyle {
                                    font_size: 30.,
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

            commands.entity(army_settings_nodes.battalions_row).with_children(|parent| {
                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Battalion".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Battalion".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Battalion".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

            let mut company1 = Entity::PLACEHOLDER;
            let mut company2 = Entity::PLACEHOLDER;
            let mut company3 = Entity::PLACEHOLDER;
            let mut company4 = Entity::PLACEHOLDER;
            let mut company5 = Entity::PLACEHOLDER;
            let mut company6 = Entity::PLACEHOLDER;
            let mut company7 = Entity::PLACEHOLDER;
            let mut company8 = Entity::PLACEHOLDER;
            let mut company9 = Entity::PLACEHOLDER;

            let mut company_dropdown1 = Entity::PLACEHOLDER;
            let mut company_dropdown2 = Entity::PLACEHOLDER;
            let mut company_dropdown3 = Entity::PLACEHOLDER;
            let mut company_dropdown4 = Entity::PLACEHOLDER;
            let mut company_dropdown5 = Entity::PLACEHOLDER;
            let mut company_dropdown6 = Entity::PLACEHOLDER;
            let mut company_dropdown7 = Entity::PLACEHOLDER;
            let mut company_dropdown8 = Entity::PLACEHOLDER;
            let mut company_dropdown9 = Entity::PLACEHOLDER;

            let mut company_types: Vec<String> = Vec::new();

            let mut company_index: LimitedNumber<1, 3> = LimitedNumber::new();
            let mut battalion_index: LimitedNumber<1, 3> = LimitedNumber::new();
            company_index.set_value(0);

            for _i in 0..9 {
                if company_index.next() {
                    battalion_index.next();
                }

                let squad_id = (
                    current_regiment,
                    battalion_index.get_value(),
                    company_index.get_value(),
                    1,
                    1,
                );

                if let Some(_) = team_army.regular_squads.get(&squad_id) {
                    company_types.push("Regular".to_string());
                } else if let Some(_) = team_army.shock_squads.get(&squad_id) {
                    company_types.push("Shock".to_string());
                } else if let Some(_) = team_army.armored_squads.get(&squad_id) {
                    company_types.push("Armored".to_string());
                }
            }

            commands.entity(army_settings_nodes.companies_row).with_children(|parent| {
                company1 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((0, (current_regiment, 1, 1)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((0, (current_regiment,1,1)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown1 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[0].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company2 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((1, (current_regiment, 1, 2)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((1, (current_regiment, 1, 2)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown2 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[1].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company3 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((2, (current_regiment, 1, 3)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((2, (current_regiment, 1, 3)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown3 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[2].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company4 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((3, (current_regiment, 2, 1)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((3, (current_regiment, 2, 1)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown4 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[3].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company5 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((4, (current_regiment, 2, 2)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((4, (current_regiment, 2, 2)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown5 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[4].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company6 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((5, (current_regiment, 2, 3)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((5, (current_regiment, 2, 3)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown6 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[5].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company7 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.5
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((6, (current_regiment, 3, 1)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((6, (current_regiment, 3, 1)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown7 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[6].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company8 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((7, (current_regiment, 3, 2)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((7, (current_regiment, 3, 2)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown8 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[7].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();

                company9 = parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .insert(
                    ButtonAction{action: Actions::SetupCompany((8, (current_regiment, 3, 3)))}
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Company".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenCompanyTypes((8, (current_regiment, 3, 3)))}
                    )
                    .with_children(|button_parent| {
                        company_dropdown9 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: company_types[8].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                })
                .id();
            });

            army_settings_nodes.company_buttons.2 = vec![
                company1,
                company2,
                company3,
                company4,
                company5,
                company6,
                company7,
                company8,
                company9,
            ];

            army_settings_nodes.company_type_dropdown_lists.1 = vec![
                company_dropdown1,
                company_dropdown2,
                company_dropdown3,
                company_dropdown4,
                company_dropdown5,
                company_dropdown6,
                company_dropdown7,
                company_dropdown8,
                company_dropdown9,
            ];
        }
    }
}

pub fn setup_company(
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut event_reader: EventReader<SetupCompanyEvent>,
    player_data: Res<PlayerData>,
    armies: Res<Armies>,
){
    for event in event_reader.read() {
        army_settings_nodes.squad_specialization_dropdown_lists.0 = -1;

        if event.0.0 == army_settings_nodes.company_buttons.0 {
            army_settings_nodes.company_buttons.0 = -1;
            commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
            commands.entity(army_settings_nodes.squads_row).despawn_descendants();

            if commands.get_entity(army_settings_nodes.company_buttons.1).is_some() {
                commands.entity(army_settings_nodes.company_buttons.1).despawn_recursive();
            }
        } else {
            for company_dropdown in army_settings_nodes.company_type_dropdown_lists.1.iter() {
                commands.entity(*company_dropdown).despawn_descendants();
            }

            army_settings_nodes.company_type_dropdown_lists.0 = -1;

            army_settings_nodes.company_buttons.0 = event.0.0;

            commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
            commands.entity(army_settings_nodes.squads_row).despawn_descendants();

            if commands.get_entity(army_settings_nodes.company_buttons.1).is_some() {
                commands.entity(army_settings_nodes.company_buttons.1).despawn_recursive();
            }

            let mut highlighter = Entity::PLACEHOLDER;

            commands.entity(army_settings_nodes.company_buttons.2[event.0.0 as usize]).with_children(|parent| {
                highlighter = parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size * 1.),
                            top: Val::Px(0.),
                            left: Val::Px(0.),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0., 1., 0., 0.25).into(),
                        ..default()
                    }
                ).id();
            });

            army_settings_nodes.company_buttons.1 = highlighter;

            commands.entity(army_settings_nodes.platoons_row).with_children(|parent| {
                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. / 2. - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Platoon".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. / 2. - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Platoon".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

                parent.spawn(
                    NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. / 2. - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Platoon".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
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

            let mut squads = vec![
                (0, (event.0.1.0, event.0.1.1, event.0.1.2, 1, 1), CompanyTypes::None),
                (1, (event.0.1.0, event.0.1.1, event.0.1.2, 1, 2), CompanyTypes::None),
                (2, (event.0.1.0, event.0.1.1, event.0.1.2, 1, 3), CompanyTypes::None),

                (3, (event.0.1.0, event.0.1.1, event.0.1.2, 2, 1), CompanyTypes::None),
                (4, (event.0.1.0, event.0.1.1, event.0.1.2, 2, 2), CompanyTypes::None),
                (5, (event.0.1.0, event.0.1.1, event.0.1.2, 2, 3), CompanyTypes::None),

                (6, (event.0.1.0, event.0.1.1, event.0.1.2, 3, 1), CompanyTypes::None),
                (7, (event.0.1.0, event.0.1.1, event.0.1.2, 3, 2), CompanyTypes::None),
                (8, (event.0.1.0, event.0.1.1, event.0.1.2, 3, 3), CompanyTypes::None),
            ];

            let mut current_specializations: Vec<String> = Vec::new();

            if let Some(team_army) = armies.0.get(&player_data.team) {
                if let Some(_regular_squad) = team_army.regular_squads.get(&squads[0].1) {
                    squads[0].2 = CompanyTypes::Regular;
                    squads[1].2 = CompanyTypes::Regular;
                    squads[2].2 = CompanyTypes::Regular;
                } else if let Some(_shock_squad) = team_army.shock_squads.get(&squads[0].1) {
                    squads[0].2 = CompanyTypes::Shock;
                    squads[1].2 = CompanyTypes::Shock;
                    squads[2].2 = CompanyTypes::Shock;
                } else if let Some(_armored_squad) = team_army.armored_squads.get(&squads[0].1) {
                    squads[0].2 = CompanyTypes::Armored;
                    squads[1].2 = CompanyTypes::Armored;
                    squads[2].2 = CompanyTypes::Armored;
                }

                if let Some(_regular_squad) = team_army.regular_squads.get(&squads[3].1) {
                    squads[3].2 = CompanyTypes::Regular;
                    squads[4].2 = CompanyTypes::Regular;
                    squads[5].2 = CompanyTypes::Regular;
                } else if let Some(_shock_squad) = team_army.shock_squads.get(&squads[3].1) {
                    squads[3].2 = CompanyTypes::Shock;
                    squads[4].2 = CompanyTypes::Shock;
                    squads[5].2 = CompanyTypes::Shock;
                } else if let Some(_armored_squad) = team_army.armored_squads.get(&squads[3].1) {
                    squads[3].2 = CompanyTypes::Armored;
                    squads[4].2 = CompanyTypes::Armored;
                    squads[5].2 = CompanyTypes::Armored;
                }

                if let Some(_regular_squad) = team_army.regular_squads.get(&squads[6].1) {
                    squads[6].2 = CompanyTypes::Regular;
                    squads[7].2 = CompanyTypes::Regular;
                    squads[8].2 = CompanyTypes::Regular;
                } else if let Some(_shock_squad) = team_army.shock_squads.get(&squads[6].1) {
                    squads[6].2 = CompanyTypes::Shock;
                    squads[7].2 = CompanyTypes::Shock;
                    squads[8].2 = CompanyTypes::Shock;
                } else if let Some(_armored_squad) = team_army.armored_squads.get(&squads[6].1) {
                    squads[6].2 = CompanyTypes::Armored;
                    squads[7].2 = CompanyTypes::Armored;
                    squads[8].2 = CompanyTypes::Armored;
                }

                for squad_id in squads.iter() {
                    match squad_id.2 {
                        CompanyTypes::Regular => {
                            if let Some(squad) = team_army.regular_squads.get(&squad_id.1) {
                                match squad.1.as_str() {
                                    "atgm" => {
                                        current_specializations.push("ATGM".to_string());
                                    }
                                    "sniperr" => {
                                        current_specializations.push("Sniper".to_string());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CompanyTypes::Shock => {
                            if let Some(squad) = team_army.shock_squads.get(&squad_id.1) {
                                match squad.1.as_str() {
                                    "lat" => {
                                        current_specializations.push("LAT".to_string());
                                    }
                                    "snipers" => {
                                        current_specializations.push("Sniper".to_string());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        CompanyTypes::Armored => {
                            if let Some(squad) = team_army.armored_squads.get(&squad_id.1) {
                                match squad.1.as_str() {
                                    "tank" => {
                                        current_specializations.push("Tank".to_string());
                                    }
                                    "ifv" => {
                                        current_specializations.push("IFV".to_string());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            let mut squad1 = Entity::PLACEHOLDER;
            let mut squad2 = Entity::PLACEHOLDER;
            let mut squad3 = Entity::PLACEHOLDER;
            let mut squad4 = Entity::PLACEHOLDER;
            let mut squad5 = Entity::PLACEHOLDER;
            let mut squad6 = Entity::PLACEHOLDER;
            let mut squad7 = Entity::PLACEHOLDER;
            let mut squad8 = Entity::PLACEHOLDER;
            let mut squad9 = Entity::PLACEHOLDER;

            commands.entity(army_settings_nodes.squads_row).with_children(|parent| {
                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[0])}
                    )
                    .with_children(|button_parent| {
                        squad1 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[0].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[1])}
                    )
                    .with_children(|button_parent| {
                        squad2 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[1].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.5
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[2])}
                    )
                    .with_children(|button_parent| {
                        squad3 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[2].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                                - ui_button_nodes.button_size * 2.
                                - ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[3])}
                    )
                    .with_children(|button_parent| {
                        squad4 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[3].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                - ui_button_nodes.button_size * 2. / 2.
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[4])}
                    )
                    .with_children(|button_parent| {
                        squad5 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[4].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[5])}
                    )
                    .with_children(|button_parent| {
                        squad6 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[5].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.5
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "1st Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[6])}
                    )
                    .with_children(|button_parent| {
                        squad7 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[6].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "2nd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[7])}
                    )
                    .with_children(|button_parent| {
                        squad8 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[7].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });

                parent.spawn(
                    ButtonBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2.),
                            height: Val::Px(ui_button_nodes.button_size),
                            top: Val::Px(army_settings_nodes.land_army_settings_node_height as f32 / 6. * 0.4 - ui_button_nodes.button_size / 2.),
                            left: Val::Px(
                                army_settings_nodes.land_army_settings_node_width as f32 * 0.8 / 2.
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.5
                                + ui_button_nodes.button_size * 2.
                                + ui_button_nodes.button_size * 0.1
                                + ui_button_nodes.button_size * 2. / 2.
                                + ui_button_nodes.button_size * 0.1
                            ),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }
                )
                .with_children(|bar_parent| {
                    bar_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: "3rd Squad".to_string(),
                                style: TextStyle {
                                    font_size: 30.,
                                    ..default()
                                },
                                ..default()
                            }],
                            justify: JustifyText::Center,
                            ..default()
                        },
                        ..default()
                    });
                })
                .with_children(|bar_parent| {
                    bar_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size * 1.1),
                                left: Val::Px(0.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{action: Actions::OpenSquadSpecializations(squads[8])}
                    )
                    .with_children(|button_parent| {
                        squad9 = button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: current_specializations[8].clone(),
                                    style: TextStyle {
                                        font_size: 30.,
                                        ..default()
                                    },
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default()
                            },
                            ..default()
                        }).id();
                    });
                });
            });

            army_settings_nodes.squad_specialization_dropdown_lists.1 = vec![
                squad1,
                squad2,
                squad3,
                squad4,
                squad5,
                squad6,
                squad7,
                squad8,
                squad9,
            ];
        }

        // if army_settings_nodes.last_battalion_button_index != -1 {
        //     let index = army_settings_nodes.last_battalion_button_index as usize;

        //     if event.0.0 != army_settings_nodes.last_battalion_button_index &&
        //     army_settings_nodes.company_buttons[index].1.get_value() == 1 {
        //         army_settings_nodes.company_buttons[index].1.next();
        //     }
        // }

        // army_settings_nodes.last_battalion_button_index = event.0.0;

        // if army_settings_nodes.company_buttons[event.0.0 as usize].1.next() {
        //     commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
        //     commands.entity(army_settings_nodes.squads_row).despawn_descendants();
        // }
        // else {
        //     commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
        //     commands.entity(army_settings_nodes.squads_row).despawn_descendants();

        //     let mut company_id: LimitedNumber<1, 3> = LimitedNumber::new();
        //     let mut platoon_id: LimitedNumber<1, 3> = LimitedNumber::new();
        //     platoon_id.set_value(0);
    
        //     match army_settings_nodes.batallion_type_dropdown_lists[event.0.0 as usize].1 {
        //         CompanyTypes::Regular => {
        //             let mut company_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..3 {
        //                 commands.entity(army_settings_nodes.platoons_row).with_children(|parent| {
        //                     company_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
                
        //             let mut platoon_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..9 {
        //                 commands.entity(army_settings_nodes.squads_row).with_children(|parent| {
        //                     platoon_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 18) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
    
        //             for placeholder in company_placeholders {
        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Company".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 20.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });
        //             }

        //             let mut counter = 0;
        //             let platoon_start_index = event.0.0 * 9;

        //             army_settings_nodes.platoon_specialization_dropdown_lists.clear();

        //             for placeholder in platoon_placeholders {
        //                 if platoon_id.next() {
        //                     company_id.next();
        //                 }

        //                 if army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 != CompanyTypes::Regular {
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0 = ("atgm".to_string(), "ATGM".to_string());
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 = CompanyTypes::Regular;
        //                 }

        //                 let current_platoon_specialization = army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0.clone();

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Platoon".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 10.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     }).insert(ButtonAction{
        //                         action: Actions::OpenPlatoonSpecializations((
        //                             counter, (event.0.1.0, event.0.1.1, event.0.1.2, company_id.get_value(), platoon_id.get_value()), CompanyTypes::Regular
        //                         ))}
        //                     )
        //                     .with_children(|button_parent| {
        //                         army_settings_nodes.platoon_specialization_dropdown_lists.push((
        //                             button_parent.spawn(TextBundle {
        //                                 text: Text{
        //                                     sections: vec![TextSection {
        //                                         value: current_platoon_specialization.1.clone(),
        //                                         style: TextStyle {
        //                                             font_size: 10.,
        //                                             ..default()
        //                                         },
        //                                         ..default()
        //                                     }],
        //                                     justify: JustifyText::Center,
        //                                     ..default()
                    
        //                                 },
        //                                 style: Style {
        //                                     justify_content: JustifyContent::Center,
        //                                     align_items: AlignItems::Center,
        //                                     ..default()
        //                                 },
        //                                 ..default()
        //                             }).id(),
        //                             current_platoon_specialization.0.clone(),
        //                             LimitedNumber::new()
        //                         ));
        //                     });
        //                 });

        //                 counter += 1;
        //             }
        //         },
        //         CompanyTypes::Shock => {
        //             let mut company_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..3 {
        //                 commands.entity(army_settings_nodes.platoons_row).with_children(|parent| {
        //                     company_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
                
        //             let mut platoon_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..9 {
        //                 commands.entity(army_settings_nodes.squads_row).with_children(|parent| {
        //                     platoon_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 18) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
    
        //             for placeholder in company_placeholders {
        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Company".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 20.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });
        //             }

        //             let mut counter = 0;
        //             let platoon_start_index = event.0.0 * 9;

        //             army_settings_nodes.platoon_specialization_dropdown_lists.clear();

        //             for placeholder in platoon_placeholders {
        //                 if platoon_id.next() {
        //                     company_id.next();
        //                 }

        //                 if army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 != CompanyTypes::Shock {
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0 = ("lat".to_string(), "LAT".to_string());
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 = CompanyTypes::Shock;
        //                 }

        //                 let current_platoon_specialization = army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0.clone();

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Platoon".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 10.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     }).insert(ButtonAction{
        //                         action: Actions::OpenPlatoonSpecializations((
        //                             counter, (event.0.1.0, event.0.1.1, event.0.1.2, company_id.get_value(), platoon_id.get_value()), CompanyTypes::Shock
        //                         ))}
        //                     )
        //                     .with_children(|button_parent| {
        //                         army_settings_nodes.platoon_specialization_dropdown_lists.push((
        //                             button_parent.spawn(TextBundle {
        //                                 text: Text{
        //                                     sections: vec![TextSection {
        //                                         value: current_platoon_specialization.1.clone(),
        //                                         style: TextStyle {
        //                                             font_size: 10.,
        //                                             ..default()
        //                                         },
        //                                         ..default()
        //                                     }],
        //                                     justify: JustifyText::Center,
        //                                     ..default()
                    
        //                                 },
        //                                 style: Style {
        //                                     justify_content: JustifyContent::Center,
        //                                     align_items: AlignItems::Center,
        //                                     ..default()
        //                                 },
        //                                 ..default()
        //                             }).id(),
        //                             current_platoon_specialization.0.clone(),
        //                             LimitedNumber::new()
        //                         ));
        //                     });
        //                 });

        //                 counter += 1;
        //             }
        //         },
        //         CompanyTypes::Armored => {
        //             let mut company_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..3 {
        //                 commands.entity(army_settings_nodes.platoons_row).with_children(|parent| {
        //                     company_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
                
        //             let mut platoon_placeholders: Vec<Entity> = Vec::new();
        //             for _i in 0..9 {
        //                 commands.entity(army_settings_nodes.squads_row).with_children(|parent| {
        //                     platoon_placeholders.push(
        //                         parent.spawn(NodeBundle{
        //                             style: Style {
        //                                 position_type: PositionType::Relative,
        //                                 width: Val::Px((army_settings_nodes.land_army_settings_node_width / 18) as f32 - ui_button_nodes.margin * 2.),
        //                                 height: Val::Px((army_settings_nodes.land_army_settings_node_height / 6) as f32 - ui_button_nodes.margin * 2.),
        //                                 margin: UiRect {
        //                                     left: Val::Px(ui_button_nodes.margin),
        //                                     right: Val::Px(ui_button_nodes.margin),
        //                                     top: Val::Px(ui_button_nodes.margin),
        //                                     bottom: Val::Px(ui_button_nodes.margin),
        //                                 },
        //                                 justify_content: JustifyContent::Center,
        //                                 align_items: AlignItems::Center,
        //                                 flex_direction: FlexDirection::Column,
        //                                 ..default()
        //                             },
        //                             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //                             ..default()
        //                         }).id()
        //                     );
        //                 });
        //             }
    
        //             for placeholder in company_placeholders {
        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Company".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 20.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });
        //             }

        //             let mut counter = 0;
        //             let platoon_start_index = event.0.0 * 9;

        //             army_settings_nodes.platoon_specialization_dropdown_lists.clear();

        //             for placeholder in platoon_placeholders {
        //                 if platoon_id.next() {
        //                     company_id.next();
        //                 }

        //                 if army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 != CompanyTypes::Armored {
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0 = ("tank".to_string(), "Tank".to_string());
        //                     army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].1 = CompanyTypes::Armored;
        //                 }

        //                 let current_platoon_specialization = army_settings_nodes.platoon_specialization_cache[(platoon_start_index + counter) as usize].0.clone();

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     })
        //                     .with_children(|button_parent| {
        //                         button_parent.spawn(TextBundle {
        //                             text: Text{
        //                                 sections: vec![TextSection {
        //                                     value: "Platoon".to_string(),
        //                                     style: TextStyle {
        //                                         font_size: 10.,
        //                                         ..default()
        //                                     },
        //                                     ..default()
        //                                 }],
        //                                 justify: JustifyText::Center,
        //                                 ..default() 
        //                             },
        //                             ..default()
        //                         });
        //                     });
        //                 });

        //                 commands.entity(placeholder).with_children(|parent| {
        //                     parent.spawn(ButtonBundle{
        //                         style: Style {
        //                             position_type: PositionType::Relative,
        //                             width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                             height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                             margin: UiRect {
        //                                 left: Val::Px(ui_button_nodes.margin),
        //                                 right: Val::Px(ui_button_nodes.margin),
        //                                 top: Val::Px(ui_button_nodes.margin),
        //                                 bottom: Val::Px(ui_button_nodes.margin),
        //                             },
        //                             justify_content: JustifyContent::Center,
        //                             align_items: AlignItems::Center,
        //                             ..default()
        //                         },
        //                         background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                         ..default()
        //                     }).insert(ButtonAction{
        //                         action: Actions::OpenPlatoonSpecializations((
        //                             counter, (event.0.1.0, event.0.1.1, event.0.1.2, company_id.get_value(), platoon_id.get_value()), CompanyTypes::Armored
        //                         ))}
        //                     )
        //                     .with_children(|button_parent| {
        //                         army_settings_nodes.platoon_specialization_dropdown_lists.push((
        //                             button_parent.spawn(TextBundle {
        //                                 text: Text{
        //                                     sections: vec![TextSection {
        //                                         value: current_platoon_specialization.1.clone(),
        //                                         style: TextStyle {
        //                                             font_size: 10.,
        //                                             ..default()
        //                                         },
        //                                         ..default()
        //                                     }],
        //                                     justify: JustifyText::Center,
        //                                     ..default()
                    
        //                                 },
        //                                 style: Style {
        //                                     justify_content: JustifyContent::Center,
        //                                     align_items: AlignItems::Center,
        //                                     ..default()
        //                                 },
        //                                 ..default()
        //                             }).id(),
        //                             current_platoon_specialization.0.clone(),
        //                             LimitedNumber::new()
        //                         ));
        //                     });
        //                 });

        //                 counter += 1;
        //             }
        //         },
        //         _ => {},
        //     }
        // }
    }
}

pub fn open_specializations_dropdown_list(
    ui_button_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    mut event_reader: EventReader<OpenSquadSpecializationsEvent>,
    specializations: Res<Specializations>,
    mut dropdown_list_entity: Local<Option<Entity>>,
    player_data: Res<PlayerData>,
    armies: Res<Armies>,
){
    for event in event_reader.read() {        
        if event.0.0 == army_settings_nodes.squad_specialization_dropdown_lists.0 {
            army_settings_nodes.squad_specialization_dropdown_lists.0 = -1;

            if let Some(dropdown_list) = *dropdown_list_entity {
                if commands.get_entity(dropdown_list).is_some() {
                    commands.entity(dropdown_list).despawn_recursive();
                }
            }

            *dropdown_list_entity = None;
        } else {
            army_settings_nodes.squad_specialization_dropdown_lists.0 = event.0.0;

            if let Some(dropdown_list) = *dropdown_list_entity {
                if commands.get_entity(dropdown_list).is_some() {
                    commands.entity(dropdown_list).despawn_recursive();
                }
            }

            *dropdown_list_entity = None;

            let mut dropdown_list = Entity::PLACEHOLDER;

            let mut available_specializations: Vec<(String, String)> = Vec::new();

            if let Some(team_army) = armies.0.get(&player_data.team) {
                if let Some(_squad) = team_army.regular_squads.get(&event.0.1) {
                    available_specializations = specializations.regular.clone();
                } else if let Some(_squad) = team_army.shock_squads.get(&event.0.1) {
                    available_specializations = specializations.shock.clone();
                } else if let Some(_squad) = team_army.armored_squads.get(&event.0.1) {
                    available_specializations = specializations.armored.clone();
                }
            }

            commands.entity(army_settings_nodes.squad_specialization_dropdown_lists.1[event.0.0 as usize]).with_children(|parent| {
                dropdown_list = parent.spawn(
                NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2. * 1.1),
                            height: Val::Px(ui_button_nodes.button_size * 1.1),
                            top: Val::Px(ui_button_nodes.button_size * 0.2),
                            left: Val::Px((ui_button_nodes.button_size * 2. - ui_button_nodes.button_size * 2. * 1.1) / 2. - ui_button_nodes.button_size * 2. / 2.),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 0.25).into(),
                        ..default()
                    }
                )
                .with_children(|dropdown_list_parent| {
                    dropdown_list_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size / 10.),
                                left: Val::Px((ui_button_nodes.button_size * 2. * 1.1 - ui_button_nodes.button_size * 2.) / 2.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{
                            action: Actions::ChooseSquadSpecialization((available_specializations[0].clone(), event.0.1, event.0.0, event.0.2))
                        }
                    )
                    .with_children(|list_parent| {
                        list_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: available_specializations[0].1.clone(),
                                    style: TextStyle {
                                        font_size: 30.,
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

                    dropdown_list_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size / 10. + ui_button_nodes.button_size * 0.4 + ui_button_nodes.button_size / 10.),
                                left: Val::Px((ui_button_nodes.button_size * 2. * 1.1 - ui_button_nodes.button_size * 2.) / 2.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{
                            action: Actions::ChooseSquadSpecialization((available_specializations[1].clone(), event.0.1, event.0.0, event.0.2))
                        }
                    )
                    .with_children(|list_parent| {
                        list_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: available_specializations[1].1.clone(),
                                    style: TextStyle {
                                        font_size: 30.,
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
                })
                .id();
            });

            *dropdown_list_entity = Some(dropdown_list);
        }

        // if army_settings_nodes.last_platoon_specialization_dropdown_list_index != -1 {
        //     let index = army_settings_nodes.last_platoon_specialization_dropdown_list_index as usize;

        //     if event.0.0 != army_settings_nodes.last_platoon_specialization_dropdown_list_index &&
        //     army_settings_nodes.platoon_specialization_dropdown_lists[index].2.get_value() == 1 {
        //         army_settings_nodes.platoon_specialization_dropdown_lists[index].2.next();
        //     }
        // }

        // army_settings_nodes.last_platoon_specialization_dropdown_list_index = event.0.0;

        // if army_settings_nodes.platoon_specialization_dropdown_lists[event.0.0 as usize].2.next() {
        //     commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.0 as usize].0).despawn_descendants();
        // }
        // else{
        //     for dropdown_list in army_settings_nodes.platoon_specialization_dropdown_lists.clone() {
        //         commands.entity(dropdown_list.0).despawn_descendants();
        //     }

        //     let mut current_specializations: Vec<(String, String)> = Vec::new();
        //     match event.0.2 {
        //         CompanyTypes::Regular => {
        //             current_specializations = specializations.regular.clone();
        //         },
        //         CompanyTypes::Shock => {
        //             current_specializations = specializations.shock.clone();
        //         },
        //         CompanyTypes::Armored => {
        //             current_specializations = specializations.armored.clone();
        //         },
        //         _ => {},
        //     }

        //     let mut dropdown_list_node= Entity::PLACEHOLDER;
        //     commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.0 as usize].0).with_children(|parent| {
        //         dropdown_list_node = parent.spawn(NodeBundle{
        //             style: Style {
        //                 position_type: PositionType::Absolute,
        //                 top: Val::Px(ui_button_nodes.button_size / 4.),
        //                 width: Val::Px(ui_button_nodes.button_size),
        //                 height: Val::Px(ui_button_nodes.button_size / 4. * (current_specializations.len() as f32 + 2.)),
        //                 flex_direction: FlexDirection::Column,
        //                 justify_content: JustifyContent::Center,
        //                 align_items: AlignItems::Center,
        //                 ..default()
        //             },
        //             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //             ..default()
        //         }).id();
        //     });

        //     for specialization in current_specializations {
        //         commands.entity(dropdown_list_node).with_children(|parent| {
        //             parent.spawn(ButtonBundle{
        //                 style: Style {
        //                     position_type: PositionType::Relative,
        //                     width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                     height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                     margin: UiRect {
        //                         left: Val::Px(ui_button_nodes.margin),
        //                         right: Val::Px(ui_button_nodes.margin),
        //                         top: Val::Px(ui_button_nodes.margin),
        //                         bottom: Val::Px(ui_button_nodes.margin),
        //                     },
        //                     justify_content: JustifyContent::Center,
        //                     align_items: AlignItems::Center,
        //                     ..default()
        //                 },
        //                 background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                 ..default()
        //             }).insert(ButtonAction{
        //                 action: Actions::ChooseSquadSpecialization(
        //                     (specialization.clone(), (event.0.1.0, event.0.1.1, event.0.1.2, event.0.1.3, event.0.1.4), event.0.0, event.0.2)
        //                 ),
        //             })
        //             .with_children(|button_parent| {
        //                 button_parent.spawn(TextBundle {
        //                     text: Text{
        //                         sections: vec![TextSection {
        //                             value: specialization.1.clone(),
        //                             style: TextStyle {
        //                                 font_size: 10.,
        //                                 ..default()
        //                             },
        //                             ..default()
        //                         }],
        //                         justify: JustifyText::Center,
        //                         ..default()
        
        //                     },
        //                     ..default()
        //                 });
        //             });
        //         });
        //     }
        // }
    }
}

pub fn choose_squad_specialization(
    mut armies: ResMut<Armies>,
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<ChooseSquadSpecializationEvent>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for event in event_reader.read() {
        army_settings_nodes.squad_specialization_dropdown_lists.0 = -1;

        commands.entity(army_settings_nodes.squad_specialization_dropdown_lists.1[event.0.2 as usize]).despawn_descendants();
        commands.entity(army_settings_nodes.squad_specialization_dropdown_lists.1[event.0.2 as usize]).insert(
        Text::from_section(
            event.0.0.1.clone(),
            TextStyle {
                font_size: 30.,
                ..default()
            })
        );

        if let Some(team_army) = armies.0.get_mut(&player_data.team) {
            if let Some(squad) = team_army.regular_squads.get_mut(&event.0.1) {
                squad.1 = event.0.0.0.clone();
            } else if let Some(squad) = team_army.shock_squads.get_mut(&event.0.1) {
                squad.1 = event.0.0.0.clone();
            } else if let Some(squad) = team_army.armored_squads.get_mut(&event.0.1) {
                squad.1 = event.0.0.0.clone();
            }

            if matches!(network_status.0, NetworkStatuses::Client) {
                let mut regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))> = Vec::new();
                let mut shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))> = Vec::new();
                let mut armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))> = Vec::new();
                let mut artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity) = (Vec::new(), Entity::PLACEHOLDER);
                let mut engineers: Vec<(i32, ((Option<Entity>, String), Entity))> = Vec::new();

                for reg_p in team_army.regular_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in reg_p.1.0.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                            soldiers.push(*server_entity);
                        }
                    }

                    for specialist in reg_p.1.0.0.1.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                            specialists.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&reg_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    regular_platoons.push((*reg_p.0, (
                        SerializableRegularSquad((
                            soldiers,
                            specialists,
                        )),
                        reg_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for shock_p in team_army.shock_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in shock_p.1.0.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                            soldiers.push(*server_entity);
                        }
                    }

                    for specialist in shock_p.1.0.0.1.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                            specialists.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&shock_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    shock_platoons.push((*shock_p.0, (
                        SerializableShockSquad((
                            soldiers,
                            specialists,
                        )),
                        shock_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for arm_p in team_army.armored_squads.iter() {
                    let mut vehicles: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for vehicle in arm_p.1.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(vehicle) {
                            vehicles.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&arm_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    armored_platoons.push((*arm_p.0, (
                        SerializableArmoredSquad(
                            vehicles,
                        ),
                        arm_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for art in team_army.artillery_units.0.iter() {
                    let mut art_option = None;
                    if let Some(some_unit) = art.1.0.0 {
                        if let Some(client_entity) = entity_maps.client_to_server.get(&some_unit) {
                            art_option = Some(client_entity);
                        }
                    }

                    artillery_units.0.push((*art.0, ((art_option.copied(), art.1.0.1.clone()), art.1.1)));
                }

                for eng in team_army.engineers.iter() {
                    let mut eng_option = None;
                    if let Some(some_unit) = eng.1.0.0 {
                        if let Some(client_entity) = entity_maps.client_to_server.get(&some_unit) {
                            eng_option = Some(client_entity);
                        }
                    }

                    engineers.push((*eng.0, ((eng_option.copied(), eng.1.0.1.clone()), eng.1.1)));
                }

                let army = SerializableArmyObject{
                    regular_platoons,
                    shock_platoons,
                    armored_platoons,
                    artillery_units,
                    engineers,
                };

                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::ClientArmyChanged { army: army.clone()}){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            } else if matches!(network_status.0, NetworkStatuses::Host) {
                let mut regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))> = Vec::new();
                let mut shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))> = Vec::new();
                let mut armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))> = Vec::new();
                let mut artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity) = (Vec::new(), Entity::PLACEHOLDER);
                let mut engineers: Vec<(i32, ((Option<Entity>, String), Entity))> = Vec::new();

                for reg_p in team_army.regular_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in reg_p.1.0.0.0.set.iter() {
                        soldiers.push(*soldier);
                    }

                    for specialist in reg_p.1.0.0.1.set.iter() {
                        specialists.push(*specialist);
                    }

                    squad_leader = reg_p.1.2;

                    regular_platoons.push((*reg_p.0, (
                        SerializableRegularSquad((
                            soldiers,
                            specialists,
                        )),
                        reg_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for shock_p in team_army.shock_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in shock_p.1.0.0.0.set.iter() {
                        soldiers.push(*soldier);
                    }

                    for specialist in shock_p.1.0.0.1.set.iter() {
                        specialists.push(*specialist);
                    }

                    squad_leader = shock_p.1.2;

                    shock_platoons.push((*shock_p.0, (
                        SerializableShockSquad((
                            soldiers,
                            specialists,
                        )),
                        shock_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for arm_p in team_army.armored_squads.iter() {
                    let mut vehicles: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for vehicle in arm_p.1.0.0.set.iter() {
                        vehicles.push(*vehicle);
                    }

                    squad_leader = arm_p.1.2;

                    armored_platoons.push((*arm_p.0, (
                        SerializableArmoredSquad(
                            vehicles,
                        ),
                        arm_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for art in team_army.artillery_units.0.iter() {
                    artillery_units.0.push((*art.0, ((art.1.0.0, art.1.0.1.clone()), art.1.1)));
                }

                for eng in team_army.engineers.iter() {
                    engineers.push((*eng.0, ((eng.1.0.0, eng.1.0.1.clone()), eng.1.1)));
                }

                let army = SerializableArmyObject{
                    regular_platoons,
                    shock_platoons,
                    armored_platoons,
                    artillery_units,
                    engineers,
                };

                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::HostArmyChanged {
                        army: army.clone(),
                    }){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            }
        }

        // let index = army_settings_nodes.last_platoon_specialization_dropdown_list_index as usize;
        // army_settings_nodes.platoon_specialization_dropdown_lists[index].2.next();

        // let current_specialization_node_index = (((event.0.1.0 * 3 - 3 + event.0.1.1) * 3 - 3 + event.0.1.2) * 3 - 3 + event.0.1.3) * 3 - 3 + event.0.1.4 - 1;
        // army_settings_nodes.platoon_specialization_cache[current_specialization_node_index as usize].0 = event.0.0.clone();

        // match event.0.3 {
        //     CompanyTypes::Regular => {
        //         if let Some(platoon) = army.0.get_mut(&player_data.team).unwrap().regular_squads.get_mut(&event.0.1.clone()){
        //             platoon.1 = event.0.0.0.clone();
        //         }
        //         else{
        //             army.0.get_mut(&player_data.team).unwrap().regular_squads
        //             .insert(event.0.1, (RegularSquad((LimitedHashSet::new(), LimitedHashSet::new())), event.0.0.0.clone(), Entity::PLACEHOLDER));
        //         }

        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).despawn_descendants();
        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).insert(
        //             Text::from_section(
        //                 event.0.0.1.clone(),
        //                 TextStyle {
        //                     font_size: 10.,
        //                     ..default()
        //                 })
        //         );

        //         army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].1 = event.0.0.1.clone();
        //     },
        //     CompanyTypes::Shock => {
        //         if let Some(platoon) = army.0.get_mut(&player_data.team).unwrap().shock_squads.get_mut(&event.0.1.clone()){
        //             platoon.1 = event.0.0.0.clone();
        //         }
        //         else{
        //             army.0.get_mut(&player_data.team).unwrap().shock_squads
        //             .insert(event.0.1, (ShockSquad((LimitedHashSet::new(), LimitedHashSet::new())), event.0.0.0.clone(), Entity::PLACEHOLDER));
        //         }

        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).despawn_descendants();
        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).insert(
        //             Text::from_section(
        //                 event.0.0.1.clone(),
        //                 TextStyle {
        //                     font_size: 10.,
        //                     ..default()
        //                 })
        //         );

        //         army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].1 = event.0.0.1.clone();
        //     },
        //     CompanyTypes::Armored => {
        //         if let Some(platoon) = army.0.get_mut(&player_data.team).unwrap().armored_squads.get_mut(&event.0.1.clone()){
        //             platoon.1 = event.0.0.0.clone();
        //         }
        //         else{
        //             army.0.get_mut(&player_data.team).unwrap().armored_squads
        //             .insert(event.0.1, (ArmoredSquad(LimitedHashSet::new()), event.0.0.0.clone(), Entity::PLACEHOLDER));
        //         }

        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).despawn_descendants();
        //         commands.entity(army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].0).insert(
        //             Text::from_section(
        //                 event.0.0.1.clone(),
        //                 TextStyle {
        //                     font_size: 10.,
        //                     ..default()
        //                 })
        //         );

        //         army_settings_nodes.platoon_specialization_dropdown_lists[event.0.2 as usize].1 = event.0.0.1.clone();
        //     },
        //     _ => {},
        // }
    }
}

pub fn open_company_type_dropdown_list(
    ui_button_nodes: Res<UiButtonNodes>,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    mut commands: Commands,
    mut event_reader: EventReader<OpenCompanyTypesEvent>,
    mut dropdown_list_entity: Local<Option<Entity>>,
){
    for event in event_reader.read() {
        if event.0.0 == army_settings_nodes.company_type_dropdown_lists.0 {
            army_settings_nodes.company_type_dropdown_lists.0 = -1;

            if let Some(dropdown_list) = *dropdown_list_entity {
                if commands.get_entity(dropdown_list).is_some() {
                    commands.entity(dropdown_list).despawn_recursive();
                }
            }

            *dropdown_list_entity = None;
        } else {
            commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
            commands.entity(army_settings_nodes.squads_row).despawn_descendants();

            if commands.get_entity(army_settings_nodes.company_buttons.1).is_some() {
                commands.entity(army_settings_nodes.company_buttons.1).despawn();
            }

            army_settings_nodes.company_buttons.0 = -1;
            army_settings_nodes.squad_specialization_dropdown_lists.0 = -1;

            army_settings_nodes.company_type_dropdown_lists.0 = event.0.0;

            if let Some(dropdown_list) = *dropdown_list_entity {
                if commands.get_entity(dropdown_list).is_some() {
                    commands.entity(dropdown_list).despawn_recursive();
                }
            }

            *dropdown_list_entity = None;

            let mut dropdown_list = Entity::PLACEHOLDER;

            commands.entity(army_settings_nodes.company_type_dropdown_lists.1[event.0.0 as usize]).with_children(|parent| {
                dropdown_list = parent.spawn(
                NodeBundle {
                        style: Style {
                            position_type: PositionType::Absolute,
                            width: Val::Px(ui_button_nodes.button_size * 2. * 1.1),
                            height: Val::Px(ui_button_nodes.button_size * 1.6),
                            top: Val::Px(ui_button_nodes.button_size * 0.2),
                            left: Val::Px((ui_button_nodes.button_size * 2. - ui_button_nodes.button_size * 2. * 1.1) / 2. - ui_button_nodes.button_size * 2. / 2.),
                            align_content: AlignContent::Center,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            justify_items: JustifyItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 0.25).into(),
                        ..default()
                    }
                )
                .with_children(|dropdown_list_parent| {
                    dropdown_list_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size / 10.),
                                left: Val::Px((ui_button_nodes.button_size * 2. * 1.1 - ui_button_nodes.button_size * 2.) / 2.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{
                            action: Actions::ChooseCompanyType((CompanyTypes::Regular, event.0.1, event.0.0))
                        }
                    )
                    .with_children(|list_parent| {
                        list_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Regular".to_string(),
                                    style: TextStyle {
                                        font_size: 30.,
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

                    dropdown_list_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size / 10. + ui_button_nodes.button_size * 0.4 + ui_button_nodes.button_size / 10.),
                                left: Val::Px((ui_button_nodes.button_size * 2. * 1.1 - ui_button_nodes.button_size * 2.) / 2.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{
                            action: Actions::ChooseCompanyType((CompanyTypes::Shock, event.0.1, event.0.0))
                        }
                    )
                    .with_children(|list_parent| {
                        list_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Shock".to_string(),
                                    style: TextStyle {
                                        font_size: 30.,
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

                    dropdown_list_parent.spawn(
                        ButtonBundle {
                            style: Style {
                                position_type: PositionType::Absolute,
                                width: Val::Px(ui_button_nodes.button_size * 2.),
                                height: Val::Px(ui_button_nodes.button_size * 0.4),
                                top: Val::Px(ui_button_nodes.button_size / 10. + ui_button_nodes.button_size * 0.4 + ui_button_nodes.button_size / 10. + ui_button_nodes.button_size * 0.4 + ui_button_nodes.button_size / 10.),
                                left: Val::Px((ui_button_nodes.button_size * 2. * 1.1 - ui_button_nodes.button_size * 2.) / 2.),
                                align_content: AlignContent::Center,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                justify_items: JustifyItems::Center,
                                ..default()
                            },
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }
                    )
                    .insert(
                        ButtonAction{
                            action: Actions::ChooseCompanyType((CompanyTypes::Armored, event.0.1, event.0.0))
                        }
                    )
                    .with_children(|list_parent| {
                        list_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Armored".to_string(),
                                    style: TextStyle {
                                        font_size: 30.,
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
                })
                .id();
            });

            *dropdown_list_entity = Some(dropdown_list);
        }


        // if army_settings_nodes.last_battalion_type_dropdown_list_index != -1 {
        //     let index = army_settings_nodes.last_battalion_type_dropdown_list_index as usize;

        //     if event.0.0 != army_settings_nodes.last_battalion_type_dropdown_list_index &&
        //     army_settings_nodes.batallion_type_dropdown_lists[index].2.get_value() == 1{
        //         army_settings_nodes.batallion_type_dropdown_lists[index].2.next();
        //     }
        // }

        // army_settings_nodes.last_battalion_type_dropdown_list_index = event.0.0;

        // if army_settings_nodes.batallion_type_dropdown_lists[event.0.0 as usize].2.next() {
        //     commands.entity(army_settings_nodes.batallion_type_dropdown_lists[event.0.0 as usize].0).despawn_descendants();
        // } else {
        //     for dropdown_list in army_settings_nodes.batallion_type_dropdown_lists.clone() {
        //         commands.entity(dropdown_list.0).despawn_descendants();
        //     }

        //     commands.entity(army_settings_nodes.platoons_row).despawn_descendants();
        //     commands.entity(army_settings_nodes.squads_row).despawn_descendants();

        //     commands.entity(army_settings_nodes.batallion_type_dropdown_lists[event.0.0 as usize].0).with_children(|parent| {
        //         parent.spawn(NodeBundle{
        //             style: Style {
        //                 position_type: PositionType::Absolute,
        //                 top: Val::Px(ui_button_nodes.button_size / 4.),
        //                 width: Val::Px(ui_button_nodes.button_size),
        //                 height: Val::Px(ui_button_nodes.button_size / 4. * 5.),
        //                 flex_direction: FlexDirection::Column,
        //                 justify_content: JustifyContent::Center,
        //                 align_items: AlignItems::Center,
        //                 ..default()
        //             },
        //             background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
        //             ..default()
        //         })
        //         .with_children(|parent| {
        //             parent.spawn(ButtonBundle{
        //                 style: Style {
        //                     position_type: PositionType::Relative,
        //                     width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                     height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                     margin: UiRect {
        //                         left: Val::Px(ui_button_nodes.margin),
        //                         right: Val::Px(ui_button_nodes.margin),
        //                         top: Val::Px(ui_button_nodes.margin),
        //                         bottom: Val::Px(ui_button_nodes.margin),
        //                     },
        //                     justify_content: JustifyContent::Center,
        //                     align_items: AlignItems::Center,
        //                     ..default()
        //                 },
        //                 background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                 ..default()
        //             }).insert(ButtonAction{
        //                 action: Actions::ChooseCompanyType((CompanyTypes::Regular, (event.0.1.0, event.0.1.1, event.0.1.2), event.0.0)),
        //             })
        //             .with_children(|button_parent| {
        //                 button_parent.spawn(TextBundle {
        //                     text: Text{
        //                         sections: vec![TextSection {
        //                             value: "Regular".to_string(),
        //                             style: TextStyle {
        //                                 font_size: 10.,
        //                                 ..default()
        //                             },
        //                             ..default()
        //                         }],
        //                         justify: JustifyText::Center,
        //                         ..default()
        
        //                     },
        //                     ..default()
        //                 });
        //             });
    
        //             parent.spawn(ButtonBundle{
        //                 style: Style {
        //                     position_type: PositionType::Relative,
        //                     width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                     height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                     margin: UiRect {
        //                         left: Val::Px(ui_button_nodes.margin),
        //                         right: Val::Px(ui_button_nodes.margin),
        //                         top: Val::Px(ui_button_nodes.margin),
        //                         bottom: Val::Px(ui_button_nodes.margin),
        //                     },
        //                     justify_content: JustifyContent::Center,
        //                     align_items: AlignItems::Center,
        //                     ..default()
        //                 },
        //                 background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                 ..default()
        //             }).insert(ButtonAction{
        //                 action: Actions::ChooseCompanyType((CompanyTypes::Shock, (event.0.1.0, event.0.1.1, event.0.1.2), event.0.0)),
        //             })
        //             .with_children(|button_parent| {
        //                 button_parent.spawn(TextBundle {
        //                     text: Text{
        //                         sections: vec![TextSection {
        //                             value: "Shock".to_string(),
        //                             style: TextStyle {
        //                                 font_size: 10.,
        //                                 ..default()
        //                             },
        //                             ..default()
        //                         }],
        //                         justify: JustifyText::Center,
        //                         ..default()
        
        //                     },
        //                     ..default()
        //                 });
        //             });
    
        //             parent.spawn(ButtonBundle{
        //                 style: Style {
        //                     position_type: PositionType::Relative,
        //                     width: Val::Px(ui_button_nodes.button_size - ui_button_nodes.margin * 2.),
        //                     height: Val::Px((ui_button_nodes.button_size - ui_button_nodes.margin * 2.) / 4.),
        //                     margin: UiRect {
        //                         left: Val::Px(ui_button_nodes.margin),
        //                         right: Val::Px(ui_button_nodes.margin),
        //                         top: Val::Px(ui_button_nodes.margin),
        //                         bottom: Val::Px(ui_button_nodes.margin),
        //                     },
        //                     justify_content: JustifyContent::Center,
        //                     align_items: AlignItems::Center,
        //                     ..default()
        //                 },
        //                 background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
        //                 ..default()
        //             }).insert(ButtonAction{
        //                 action: Actions::ChooseCompanyType((CompanyTypes::Armored, (event.0.1.0, event.0.1.1, event.0.1.2), event.0.0)),
        //             })
        //             .with_children(|button_parent| {
        //                 button_parent.spawn(TextBundle {
        //                     text: Text{
        //                         sections: vec![TextSection {
        //                             value: "Armored".to_string(),
        //                             style: TextStyle {
        //                                 font_size: 10.,
        //                                 ..default()
        //                             },
        //                             ..default()
        //                         }],
        //                         justify: JustifyText::Center,
        //                         ..default()
        
        //                     },
        //                     ..default()
        //                 });
        //             });
        //         });
        //     });
        // }
    }
}

pub fn choose_company_type(
    mut armies: ResMut<Armies>,
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    player_data: Res<PlayerData>,
    mut event_reader: EventReader<ChooseCompanyTypeEvent>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    for event in event_reader.read() {
        if let Some(team_army) = armies.0.get_mut(&player_data.team) {
            army_settings_nodes.company_type_dropdown_lists.0 = -1;

            commands.entity(army_settings_nodes.company_type_dropdown_lists.1[event.0.2 as usize]).despawn_descendants();

            let mut company_id: LimitedNumber<1, 3> = LimitedNumber::new();
            let mut platoon_id: LimitedNumber<1, 3> = LimitedNumber::new();
            platoon_id.set_value(0);
            match event.0.0 {
                CompanyTypes::Regular => {
                    for _i in 0..9 {
                        if platoon_id.next() {
                            company_id.next();
                        }

                        team_army.shock_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.armored_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.regular_squads.insert(
                            (
                                event.0.1.0,
                                event.0.1.1,
                                event.0.1.2,
                                company_id.get_value(),
                                platoon_id.get_value(),
                            ), (RegularSquad((LimitedHashSet::new(), LimitedHashSet::new())), "atgm".to_string(), Entity::PLACEHOLDER)
                        );
                    }

                    commands.entity(army_settings_nodes.company_type_dropdown_lists.1[event.0.2 as usize]).insert(
                    Text::from_section(
                        "Regular".to_string(),
                        TextStyle {
                            font_size: 30.,
                            ..default()
                        })
                    );
                },
                CompanyTypes::Shock => {
                    for _i in 0..9 {
                        if platoon_id.next() {
                            company_id.next();
                        }

                        team_army.regular_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.armored_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.shock_squads.insert(
                            (
                                event.0.1.0,
                                event.0.1.1,
                                event.0.1.2,
                                company_id.get_value(),
                                platoon_id.get_value(),
                            ), (ShockSquad((LimitedHashSet::new(), LimitedHashSet::new())), "lat".to_string(), Entity::PLACEHOLDER)
                        );
                    }

                    commands.entity(army_settings_nodes.company_type_dropdown_lists.1[event.0.2 as usize]).insert(
                    Text::from_section(
                        "Shock".to_string(),
                        TextStyle {
                            font_size: 30.,
                            ..default()
                        })
                    );
                },
                CompanyTypes::Armored => {
                    for _i in 0..9 {
                        if platoon_id.next() {
                            company_id.next();
                        }

                        team_army.shock_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.regular_squads.remove(&(
                            event.0.1.0,
                            event.0.1.1,
                            event.0.1.2,
                            company_id.get_value(),
                            platoon_id.get_value(),
                        ));

                        team_army.armored_squads.insert(
                            (
                                event.0.1.0,
                                event.0.1.1,
                                event.0.1.2,
                                company_id.get_value(),
                                platoon_id.get_value(),
                            ), (ArmoredSquad(LimitedHashSet::new()), "tank".to_string(), Entity::PLACEHOLDER)
                        );
                    }

                    commands.entity(army_settings_nodes.company_type_dropdown_lists.1[event.0.2 as usize]).insert(
                    Text::from_section(
                        "Armored".to_string(),
                        TextStyle {
                            font_size: 30.,
                            ..default()
                        })
                    );
                },
                _ => {},
            }

            if matches!(network_status.0, NetworkStatuses::Client) {
                let mut regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))> = Vec::new();
                let mut shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))> = Vec::new();
                let mut armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))> = Vec::new();
                let mut artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity) = (Vec::new(), Entity::PLACEHOLDER);
                let mut engineers: Vec<(i32, ((Option<Entity>, String), Entity))> = Vec::new();

                for reg_p in team_army.regular_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in reg_p.1.0.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                            soldiers.push(*server_entity);
                        }
                    }

                    for specialist in reg_p.1.0.0.1.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                            specialists.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&reg_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    regular_platoons.push((*reg_p.0, (
                        SerializableRegularSquad((
                            soldiers,
                            specialists,
                        )),
                        reg_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for shock_p in team_army.shock_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in shock_p.1.0.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                            soldiers.push(*server_entity);
                        }
                    }

                    for specialist in shock_p.1.0.0.1.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                            specialists.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&shock_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    shock_platoons.push((*shock_p.0, (
                        SerializableShockSquad((
                            soldiers,
                            specialists,
                        )),
                        shock_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for arm_p in team_army.armored_squads.iter() {
                    let mut vehicles: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for vehicle in arm_p.1.0.0.set.iter() {
                        if let Some(server_entity) = entity_maps.client_to_server.get(vehicle) {
                            vehicles.push(*server_entity);
                        }
                    }

                    if let Some(server_entity) = entity_maps.client_to_server.get(&arm_p.1.2) {
                        squad_leader = *server_entity;
                    }

                    armored_platoons.push((*arm_p.0, (
                        SerializableArmoredSquad(
                            vehicles,
                        ),
                        arm_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for art in team_army.artillery_units.0.iter() {
                    let mut art_option = None;
                    if let Some(some_unit) = art.1.0.0 {
                        if let Some(client_entity) = entity_maps.client_to_server.get(&some_unit) {
                            art_option = Some(client_entity);
                        }
                    }

                    artillery_units.0.push((*art.0, ((art_option.copied(), art.1.0.1.clone()), art.1.1)));
                }

                for eng in team_army.engineers.iter() {
                    let mut eng_option = None;
                    if let Some(some_unit) = eng.1.0.0 {
                        if let Some(client_entity) = entity_maps.client_to_server.get(&some_unit) {
                            eng_option = Some(client_entity);
                        }
                    }

                    engineers.push((*eng.0, ((eng_option.copied(), eng.1.0.1.clone()), eng.1.1)));
                }

                let army = SerializableArmyObject{
                    regular_platoons,
                    shock_platoons,
                    armored_platoons,
                    artillery_units,
                    engineers,
                };

                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::ClientArmyChanged { army: army.clone()}){
                        channel_id += 1;
                    } else {
                        break;
                    }
                }
            } else if matches!(network_status.0, NetworkStatuses::Host) {
                let mut regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))> = Vec::new();
                let mut shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))> = Vec::new();
                let mut armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))> = Vec::new();
                let mut artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity) = (Vec::new(), Entity::PLACEHOLDER);
                let mut engineers: Vec<(i32, ((Option<Entity>, String), Entity))> = Vec::new();

                for reg_p in team_army.regular_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in reg_p.1.0.0.0.set.iter() {
                        soldiers.push(*soldier);
                    }

                    for specialist in reg_p.1.0.0.1.set.iter() {
                        specialists.push(*specialist);
                    }

                    squad_leader = reg_p.1.2;

                    regular_platoons.push((*reg_p.0, (
                        SerializableRegularSquad((
                            soldiers,
                            specialists,
                        )),
                        reg_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for shock_p in team_army.shock_squads.iter() {
                    let mut soldiers: Vec<Entity> = Vec::new();
                    let mut specialists: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for soldier in shock_p.1.0.0.0.set.iter() {
                        soldiers.push(*soldier);
                    }

                    for specialist in shock_p.1.0.0.1.set.iter() {
                        specialists.push(*specialist);
                    }

                    squad_leader = shock_p.1.2;

                    shock_platoons.push((*shock_p.0, (
                        SerializableShockSquad((
                            soldiers,
                            specialists,
                        )),
                        shock_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for arm_p in team_army.armored_squads.iter() {
                    let mut vehicles: Vec<Entity> = Vec::new();
                    let mut squad_leader = Entity::PLACEHOLDER;

                    for vehicle in arm_p.1.0.0.set.iter() {
                        vehicles.push(*vehicle);
                    }

                    squad_leader = arm_p.1.2;

                    armored_platoons.push((*arm_p.0, (
                        SerializableArmoredSquad(
                            vehicles,
                        ),
                        arm_p.1.1.clone(),
                        squad_leader,
                    )));
                }

                for art in team_army.artillery_units.0.iter() {
                    artillery_units.0.push((*art.0, ((art.1.0.0, art.1.0.1.clone()), art.1.1)));
                }

                for eng in team_army.engineers.iter() {
                    engineers.push((*eng.0, ((eng.1.0.0, eng.1.0.1.clone()), eng.1.1)));
                }

                let army = SerializableArmyObject{
                    regular_platoons,
                    shock_platoons,
                    armored_platoons,
                    artillery_units,
                    engineers,
                };

                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::HostArmyChanged {
                        army: army.clone(),
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

pub fn toggle_production(
    mut commands: Commands,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    mut production_states: ResMut<ProductionState>,
    mut event_reader: EventReader<ToggleProductionEvent>,
    mut event_writer: (
        //EventWriter<UnsentClientMessage>,
        EventWriter<ProductionStateChanged>,
    ),
    game_stage: Res<GameStage>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    army: Res<Armies>,
    entity_maps: Res<EntityMaps>,
    mut player_data: ResMut<PlayerData>,
){
    for _event in event_reader.read() {
        match game_stage.0 {
            GameStages::GameStarted => {
                if army_settings_nodes.toggle_production_button.1.next() {
                    commands.entity(army_settings_nodes.toggle_production_button.0).insert(
                        Text::from_section(
                            "Start production".to_string(),
                            TextStyle {
                                font_size: 30.,
                                ..default()
                            })
                    );

                    production_states.is_allowed.entry(player_data.team).or_insert_with(|| false);
                    event_writer.0.send(ProductionStateChanged{ team: player_data.team, is_allowed: false });

                    if matches!(network_status.0, NetworkStatuses::Client) {
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = client.connection_mut()
                            .send_message_on(channel_id, ClientMessage::ProductionStateChanged { team: player_data.team, is_allowed: false }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
                else {
                    commands.entity(army_settings_nodes.toggle_production_button.0).insert(
                        Text::from_section(
                            "Stop production".to_string(),
                            TextStyle {
                                font_size: 30.,
                                ..default()
                            })
                    );

                    production_states.is_allowed.entry(player_data.team).or_insert_with(|| true);
                    event_writer.0.send(ProductionStateChanged{ team: player_data.team, is_allowed: true });

                    if matches!(network_status.0, NetworkStatuses::Client) {
                        let mut channel_id = 60;
                        while channel_id <= 89 {
                            if let Err(_) = client
                            .connection_mut().send_message_on(channel_id, ClientMessage::ProductionStateChanged { team: player_data.team, is_allowed: true }){
                                channel_id += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            _ => {
                army_settings_nodes.toggle_production_button.1.next();

                commands.entity(army_settings_nodes.toggle_production_button.0).insert(
                    Text::from_section(
                        "Stop production".to_string(),
                        TextStyle {
                            font_size: 30.,
                            ..default()
                        })
                );

                production_states.is_allowed.entry(player_data.team).or_insert_with(|| true);

                commands.entity(army_settings_nodes.land_army_settings_node).insert(Visibility::Hidden);
                army_settings_nodes.is_land_army_settings_visible = false;

                match network_status.0 {
                    NetworkStatuses::SinglePlayer => {
                        event_writer.0.send(ProductionStateChanged{ team: player_data.team, is_allowed: true });
                    },
                    NetworkStatuses::Host => {
                        event_writer.0.send(ProductionStateChanged{ team: player_data.team, is_allowed: true });
                        player_data.is_ready_to_start = true;
                    },
                    NetworkStatuses::Client => {
                        if let Some(army) = army.0.get(&player_data.team) {
                            let mut regular_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableRegularSquad, String, Entity))> = Vec::new();
                            let mut shock_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableShockSquad, String, Entity))> = Vec::new();
                            let mut armored_platoons: Vec<((i32, i32, i32, i32, i32), (SerializableArmoredSquad, String, Entity))> = Vec::new();
                            let mut artillery_units: (Vec<(i32, ((Option<Entity>, String), Entity))>, Entity) = (Vec::new(), Entity::PLACEHOLDER);
                            let mut engineers: Vec<(i32, ((Option<Entity>, String), Entity))> = Vec::new();
    
                            for reg_p in army.regular_squads.iter() {
                                let mut soldiers: Vec<Entity> = Vec::new();
                                let mut specialists: Vec<Entity> = Vec::new();
                                let mut squad_leader = Entity::PLACEHOLDER;
    
                                for soldier in reg_p.1.0.0.0.set.iter() {
                                    if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                                        soldiers.push(*server_entity);
                                    }
                                }
    
                                for specialist in reg_p.1.0.0.1.set.iter() {
                                    if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                                        specialists.push(*server_entity);
                                    }
                                }

                                if let Some(server_entity) = entity_maps.client_to_server.get(&reg_p.1.2) {
                                    squad_leader = *server_entity;
                                }
    
                                regular_platoons.push((*reg_p.0, (
                                    SerializableRegularSquad((
                                        soldiers,
                                        specialists,
                                    )),
                                    reg_p.1.1.clone(),
                                    squad_leader,
                                )));
                            }
    
                            for shock_p in army.shock_squads.iter() {
                                let mut soldiers: Vec<Entity> = Vec::new();
                                let mut specialists: Vec<Entity> = Vec::new();
                                let mut squad_leader = Entity::PLACEHOLDER;
    
                                for soldier in shock_p.1.0.0.0.set.iter() {
                                    if let Some(server_entity) = entity_maps.client_to_server.get(soldier) {
                                        soldiers.push(*server_entity);
                                    }
                                }
    
                                for specialist in shock_p.1.0.0.1.set.iter() {
                                    if let Some(server_entity) = entity_maps.client_to_server.get(specialist) {
                                        specialists.push(*server_entity);
                                    }
                                }

                                if let Some(server_entity) = entity_maps.client_to_server.get(&shock_p.1.2) {
                                    squad_leader = *server_entity;
                                }
    
                                shock_platoons.push((*shock_p.0, (
                                    SerializableShockSquad((
                                        soldiers,
                                        specialists,
                                    )),
                                    shock_p.1.1.clone(),
                                    squad_leader,
                                )));
                            }
    
                            for arm_p in army.armored_squads.iter() {
                                let mut vehicles: Vec<Entity> = Vec::new();
                                let mut squad_leader = Entity::PLACEHOLDER;
    
                                for vehicle in arm_p.1.0.0.set.iter() {
                                    if let Some(server_entity) = entity_maps.client_to_server.get(vehicle) {
                                        vehicles.push(*server_entity);
                                    }
                                }

                                if let Some(server_entity) = entity_maps.client_to_server.get(&arm_p.1.2) {
                                    squad_leader = *server_entity;
                                }
    
                                armored_platoons.push((*arm_p.0, (
                                    SerializableArmoredSquad(
                                        vehicles,
                                    ),
                                    arm_p.1.1.clone(),
                                    squad_leader,
                                )));
                            }
    
                            for art in army.artillery_units.0.iter() {
                                let mut art_option = None;
                                if let Some(some_unit) = art.1.0.0 {
                                    art_option = Some(some_unit);
                                }

                                artillery_units.0.push((*art.0, ((art_option, art.1.0.1.clone()), art.1.1)));
                            }
    
                            for eng in army.engineers.iter() {
                                let mut eng_option = None;
                                if let Some(some_unit) = eng.1.0.0 {
                                    eng_option = Some(some_unit);
                                }

                                engineers.push((*eng.0, ((eng_option, eng.1.0.1.clone()), eng.1.1)));
                            }

                            // let regular_platoons_clone = regular_platoons.clone();
                            // let shock_platoons_clone
                            // let armored_platoons_clone
                            // let artillery_units_clone
                            // let engineers_clone

                            let army = SerializableArmyObject{
                                regular_platoons,
                                shock_platoons,
                                armored_platoons,
                                artillery_units,
                                engineers,
                            };
                            
                            let mut channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::ArmySetupStageCompleted {
                                    army: army.clone(),
                                }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }

                            channel_id = 60;
                            while channel_id <= 89 {
                                if let Err(_) = client.connection_mut()
                                .send_message_on(channel_id, ClientMessage::ProductionStateChanged { team: player_data.team, is_allowed: true }){
                                    channel_id += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

pub fn platoon_nodes_positioning_system(
    camera_q: Query<(&Camera, &GlobalTransform, &Transform), Without<SquadLeader>>,
    mut squad_nodes_q: Query<(Entity, &mut Style, &mut SquadSelector, &mut ButtonAction, &SuppliesBarHolder),
    (Without<PlatoonSelector>, Without<CompanySelector>, Without<BattalionSelector>, Without<RegimentSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut platoon_nodes_q: Query<(Entity, &mut Style, &mut PlatoonSelector, &mut ButtonAction),
    (Without<SquadSelector>, Without<CompanySelector>, Without<BattalionSelector>, Without<RegimentSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut company_nodes_q: Query<(Entity, &mut Style, &mut CompanySelector, &mut ButtonAction),
    (Without<PlatoonSelector>, Without<SquadSelector>, Without<BattalionSelector>, Without<RegimentSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut battalion_nodes_q: Query<(Entity, &mut Style, &mut BattalionSelector, &mut ButtonAction),
    (Without<PlatoonSelector>, Without<SquadSelector>, Without<CompanySelector>, Without<RegimentSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut regiment_nodes_q: Query<(Entity, &mut Style, &mut RegimentSelector, &mut ButtonAction),
    (Without<PlatoonSelector>, Without<SquadSelector>, Without<CompanySelector>, Without<BattalionSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut brigade_nodes_q: Query<(Entity, &mut Style, &mut BrigadeSelector, &mut ButtonAction),
    (Without<PlatoonSelector>, Without<SquadSelector>, Without<CompanySelector>, Without<RegimentSelector>, Without<BattalionSelector>, Without<SuppliesBar>, Without<ArtilleryUnitSelector>)>,
    mut supply_bars_q: Query<(&mut Style, &SuppliesBar),
    (Without<PlatoonSelector>, Without<SquadSelector>, Without<CompanySelector>, Without<RegimentSelector>, Without<BattalionSelector>, Without<BrigadeSelector>, Without<ArtilleryUnitSelector>)>,
    squad_leaders_q: Query<(&Transform, &SquadLeader, &CombatComponent, Option<&SuppliesConsumerComponent>, &Visibility, Option<&SelectedUnit>), (With<SquadLeader>, Without<ArtilleryUnit>, Without<DisabledUnit>)>,
    artillery_units_q: Query<(&Transform, &CombatComponent, &SuppliesConsumerComponent, &Visibility, Option<&SelectedUnit>), (With<ArtilleryUnit>, Without<SquadLeader>, Without<DisabledUnit>)>,
    mut artillery_nodes_q: Query<(Entity, &mut Style, &mut ArtilleryUnitSelector, &mut ButtonAction, &SuppliesBarHolder),
    (Without<PlatoonSelector>, Without<CompanySelector>, Without<BattalionSelector>, Without<RegimentSelector>, Without<BrigadeSelector>, Without<SuppliesBar>, Without<SquadSelector>)>,
    army: Res<Armies>,
    other: (
        Res<OtherAssets>,
        Res<UiButtonNodes>,
    ),
    symbols_level: Res<DisplayedTacicalSymbolsLevel>,
    player_data: Res<PlayerData>,
    mut commands: Commands,
){
    let camera = camera_q.iter().next().unwrap();

    let mut platoon_leaders: HashMap<(i32, (i32, i32, i32, i32, i32)), (CompanyTypes, Vec3, i32, i32, Visibility, Option<&SelectedUnit>)> = HashMap::new();

    for leader in squad_leaders_q.iter() {
        let mut supplies_capacity = 1;
        let mut supplies_storage = 1;

        if let Some(consumer) = leader.3 {
            supplies_capacity = consumer.supplies_capacity;
            supplies_storage = consumer.supplies;
        }

        platoon_leaders.insert((leader.2.team, leader.1.0.1), (leader.1.0.0, leader.0.translation, supplies_capacity, supplies_storage, *leader.4, leader.5));
    }

    for mut node in squad_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 1 {//level 1 = squads level
            if let Some(leader) = platoon_leaders.get(&(node.2.0.0, node.2.0.1.1.clone())) {
                if leader.4 == Visibility::Hidden {
                    if node.2.0.1.2 == true {
                        node.2.0.1.2 = false;
                        commands.entity(node.0).insert(Visibility::Hidden);
                    }
                    continue;
                }

                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, leader.1) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if let Some(_selected) = leader.5 {
                        if node.2.0.1.3 == Entity::PLACEHOLDER {
                            let marker_size = other.1.button_size * 0.75;
                            commands.entity(node.0).with_children(|parent| {
                                node.2.0.1.3 = parent.spawn(
                                    NodeBundle {
                                        style: Style {
                                            position_type: PositionType::Absolute,
                                            width: Val::Px(marker_size * 1.1),
                                            height: Val::Px(marker_size * 1.1),
                                            top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                            left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                            align_content: AlignContent::Center,
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            justify_items: JustifyItems::Center,
                                            ..default()
                                        },
                                        background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                        ..default()
                                    }
                                ).id();
                            });
                        }
                    } else if node.2.0.1.3 != Entity::PLACEHOLDER {
                        commands.entity(node.2.0.1.3).despawn();

                        node.2.0.1.3 = Entity::PLACEHOLDER;
                    }

                    if node.2.0.0 == 1 {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = leader.0;
                            node.3.action = Actions::SquadSelection((node.2.0.0, (leader.0, node.2.0.1.1)));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match leader.0 {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_squad_symbol_blufor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_squad_symbol_blufor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_squad_symbol_blufor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }

                        if node.2.0.0 == player_data.team {
                            if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                let modifier: f32 = leader.3 as f32 / leader.2 as f32;
                                let width = bar.1.original_width * modifier;
                                
                                bar.0.width = Val::Px(width);
                            }
                        } else {
                            if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                bar.0.width = Val::Px(0.);
                            }
                        }
                    } else {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = leader.0;
                            node.3.action = Actions::SquadSelection((node.2.0.0, (leader.0, node.2.0.1.1)));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match leader.0 {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_squad_symbol_opfor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_squad_symbol_opfor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_squad_symbol_opfor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }

                        if node.2.0.0 == player_data.team {
                            if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                let modifier: f32 = leader.3 as f32 / leader.2 as f32;
                                let width = bar.1.original_width * modifier;
                                
                                bar.0.width = Val::Px(width);
                            }
                        } else {
                            if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                bar.0.width = Val::Px(0.);
                            }
                        }
                    }
                }
                else if node.2.0.1.2 == true {
                    node.2.0.1.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.2 == true {
                node.2.0.1.2 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.2 == true {
            node.2.0.1.2 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in platoon_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 2 {//level 2 = platoons level
            let mut company_type: CompanyTypes = CompanyTypes::None;
            let mut center: Vec3 = Vec3::ZERO;
            let mut counter = 0;

            let mut is_all_visible = true;
            let mut is_all_selected = true;
            for squad in node.2.0.1.1.iter() {
                if let Some(leader) = platoon_leaders.get(&(node.2.0.0, *squad)){
                    counter += 1;

                    company_type = leader.0;

                    center += leader.1;

                    if leader.4 == Visibility::Hidden {
                        is_all_visible = false;
                        break;
                    }

                    if leader.5.is_none() {
                        is_all_selected = false;
                    }
                }
            }

            if !is_all_visible {
                if node.2.0.1.2 == true {
                    node.2.0.1.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
                continue;
            }

            if is_all_selected {
                if node.2.0.1.3 == Entity::PLACEHOLDER {
                    let marker_size = other.1.button_size * 0.75;
                    commands.entity(node.0).with_children(|parent| {
                        node.2.0.1.3 = parent.spawn(
                            NodeBundle {
                                style: Style {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(marker_size * 1.1),
                                    height: Val::Px(marker_size * 1.1),
                                    top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    align_content: AlignContent::Center,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    justify_items: JustifyItems::Center,
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                ..default()
                            }
                        ).id();
                    });
                }
            } else if node.2.0.1.3 != Entity::PLACEHOLDER {
                commands.entity(node.2.0.1.3).despawn();

                node.2.0.1.3 = Entity::PLACEHOLDER;
            }

            center = center / counter as f32;

            if counter > 0 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, center) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if node.2.0.0 == 1 {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = company_type;
                            node.3.action = Actions::PlatoonSelection((node.2.0.0, (company_type, node.2.0.1.1.clone())));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match company_type {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_platoon_symbol_blufor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_platoon_symbol_blufor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_platoon_symbol_blufor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }
                    } else {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = company_type;
                            node.3.action = Actions::PlatoonSelection((node.2.0.0, (company_type, node.2.0.1.1.clone())));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match company_type {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_platoon_symbol_opfor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_platoon_symbol_opfor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_platoon_symbol_opfor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }
                    }
                }
                else if node.2.0.1.2 == true {
                    node.2.0.1.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.2 == true {
                node.2.0.1.2 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.2 == true {
            node.2.0.1.2 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in company_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 3 {//level 3 = company level
            let mut company_type: CompanyTypes = CompanyTypes::None;
            let mut center: Vec3 = Vec3::ZERO;
            let mut counter = 0;

            let mut is_all_visible = true;
            let mut is_all_selected = true;
            for squad in node.2.0.1.1.iter() {
                if let Some(leader) = platoon_leaders.get(&(node.2.0.0, *squad)){
                    counter += 1;

                    company_type = leader.0;

                    center += leader.1;

                    if leader.4 == Visibility::Hidden {
                        is_all_visible = false;
                        break;
                    }

                    if leader.5.is_none() {
                        is_all_selected = false;
                    }
                }
            }

            if !is_all_visible {
                if node.2.0.1.2 == true {
                    node.2.0.1.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
                continue;
            }

            if is_all_selected {
                if node.2.0.1.3 == Entity::PLACEHOLDER {
                    let marker_size = other.1.button_size * 0.75;
                    commands.entity(node.0).with_children(|parent| {
                        node.2.0.1.3 = parent.spawn(
                            NodeBundle {
                                style: Style {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(marker_size * 1.1),
                                    height: Val::Px(marker_size * 1.1),
                                    top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    align_content: AlignContent::Center,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    justify_items: JustifyItems::Center,
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                ..default()
                            }
                        ).id();
                    });
                }
            } else if node.2.0.1.3 != Entity::PLACEHOLDER {
                commands.entity(node.2.0.1.3).despawn();

                node.2.0.1.3 = Entity::PLACEHOLDER;
            }

            center = center / counter as f32;

            if counter > 0 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, center) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if node.2.0.0 == 1 {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = company_type;
                            node.3.action = Actions::CompanySelection((node.2.0.0, (company_type, node.2.0.1.1.clone())));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match company_type {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_company_symbol_blufor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_company_symbol_blufor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_company_symbol_blufor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_blufor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }
                    } else {
                        if node.2.0.1.2 == false {
                            node.2.0.1.2 = true;
                            node.2.0.1.0 = company_type;
                            node.3.action = Actions::CompanySelection((node.2.0.0, (company_type, node.2.0.1.1.clone())));
                            commands.entity(node.0).insert(Visibility::Visible);

                            match company_type {
                                CompanyTypes::Regular => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.regular_infantry_company_symbol_opfor.clone()));
                                },
                                CompanyTypes::Shock => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.shock_infantry_company_symbol_opfor.clone()));
                                },
                                CompanyTypes::Armored => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.armored_company_symbol_opfor.clone()));
                                },
                                CompanyTypes::Artillery => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::Engineer => {
                                    commands.entity(node.0).insert(UiImage::new(other.0.engineer_unit_symbol_opfor.clone()));
                                },
                                CompanyTypes::None => {},
                            }
                        }
                    }
                }
                else if node.2.0.1.2 == true {
                    node.2.0.1.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.2 == true {
                node.2.0.1.2 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.2 == true {
            node.2.0.1.2 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in battalion_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 4 {//level 4 = battalion level
            let mut center: Vec3 = Vec3::ZERO;
            let mut counter = 0;

            let mut battalion: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

            let mut is_all_visible = true;
            let mut is_all_selected = true;
            for squad in node.2.0.1.0.iter() {
                if let Some(leader) = platoon_leaders.get(&(node.2.0.0, squad.1)){
                    counter += 1;

                    center += leader.1;

                    battalion.push((leader.0, squad.1));

                    if leader.4 == Visibility::Hidden {
                        is_all_visible = false;
                        break;
                    }

                    if leader.5.is_none() {
                        is_all_selected = false;
                    }
                }
            }

            if !is_all_visible {
                if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
                continue;
            }

            if is_all_selected {
                if node.2.0.1.2 == Entity::PLACEHOLDER {
                    let marker_size = other.1.button_size * 0.75;
                    commands.entity(node.0).with_children(|parent| {
                        node.2.0.1.2 = parent.spawn(
                            NodeBundle {
                                style: Style {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(marker_size * 1.1),
                                    height: Val::Px(marker_size * 1.1),
                                    top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    align_content: AlignContent::Center,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    justify_items: JustifyItems::Center,
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                ..default()
                            }
                        ).id();
                    });
                }
            } else if node.2.0.1.2 != Entity::PLACEHOLDER {
                commands.entity(node.2.0.1.2).despawn();

                node.2.0.1.2 = Entity::PLACEHOLDER;
            }

            center = center / counter as f32;

            if counter > 0 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, center) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if node.2.0.0 == 1 {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::BattalionSelection((node.2.0.0, battalion));
                            commands.entity(node.0).insert(UiImage::new(other.0.battalion_symbol_blufor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    } else {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::BattalionSelection((node.2.0.0, battalion));
                            commands.entity(node.0).insert(UiImage::new(other.0.battalion_symbol_opfor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    }
                }
                else if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.1 == true {
                node.2.0.1.1 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.1 == true {
            node.2.0.1.1 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in regiment_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 5 {//level 5 = regiment level
            let mut center: Vec3 = Vec3::ZERO;
            let mut counter = 0;

            let mut regiment: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

            let mut is_all_visible = true;
            let mut is_all_selected = true;
            for squad in node.2.0.1.0.iter() {
                if let Some(leader) = platoon_leaders.get(&(node.2.0.0 ,squad.1)){
                    counter += 1;

                    center += leader.1;

                    regiment.push((leader.0, squad.1));

                    if leader.4 == Visibility::Hidden {
                        is_all_visible = false;
                        break;
                    }

                    if leader.5.is_none() {
                        is_all_selected = false;
                    }
                }
            }

            if !is_all_visible {
                if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
                continue;
            }

            if is_all_selected {
                if node.2.0.1.2 == Entity::PLACEHOLDER {
                    let marker_size = other.1.button_size * 0.75;
                    commands.entity(node.0).with_children(|parent| {
                        node.2.0.1.2 = parent.spawn(
                            NodeBundle {
                                style: Style {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(marker_size * 1.1),
                                    height: Val::Px(marker_size * 1.1),
                                    top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    align_content: AlignContent::Center,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    justify_items: JustifyItems::Center,
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                ..default()
                            }
                        ).id();
                    });
                }
            } else if node.2.0.1.2 != Entity::PLACEHOLDER {
                commands.entity(node.2.0.1.2).despawn();

                node.2.0.1.2 = Entity::PLACEHOLDER;
            }

            center = center / counter as f32;

            if counter > 0 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, center) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if node.2.0.0 == 1 {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::RegimentSelection((node.2.0.0, regiment));
                            commands.entity(node.0).insert(UiImage::new(other.0.regiment_symbol_blufor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    } else {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::RegimentSelection((node.2.0.0, regiment));
                            commands.entity(node.0).insert(UiImage::new(other.0.regiment_symbol_opfor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    }
                }
                else if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.1 == true {
                node.2.0.1.1 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.1 == true {
            node.2.0.1.1 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in brigade_nodes_q.iter_mut() {
        if camera.2.translation.y > 100. && symbols_level.0 == 6 {//level 6 = brigade level
            let mut center: Vec3 = Vec3::ZERO;
            let mut counter = 0;

            let mut brigade: Vec<(CompanyTypes, (i32, i32, i32, i32, i32))> = Vec::new();

            let mut is_all_visible = true;
            let mut is_all_selected = true;
            for squad in node.2.0.1.0.iter() {
                if let Some(leader) = platoon_leaders.get(&(node.2.0.0, squad.1)){
                    counter += 1;

                    center += leader.1;

                    brigade.push((leader.0, squad.1));

                    if leader.4 == Visibility::Hidden {
                        is_all_visible = false;
                        break;
                    }

                    if leader.5.is_none() {
                        is_all_selected = false;
                    }
                }
            }

            if !is_all_visible {
                if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
                continue;
            }

            if is_all_selected {
                if node.2.0.1.2 == Entity::PLACEHOLDER {
                    let marker_size = other.1.button_size * 0.75;
                    commands.entity(node.0).with_children(|parent| {
                        node.2.0.1.2 = parent.spawn(
                            NodeBundle {
                                style: Style {
                                    position_type: PositionType::Absolute,
                                    width: Val::Px(marker_size * 1.1),
                                    height: Val::Px(marker_size * 1.1),
                                    top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                    align_content: AlignContent::Center,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    justify_items: JustifyItems::Center,
                                    ..default()
                                },
                                background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                ..default()
                            }
                        ).id();
                    });
                }
            } else if node.2.0.1.2 != Entity::PLACEHOLDER {
                commands.entity(node.2.0.1.2).despawn();

                node.2.0.1.2 = Entity::PLACEHOLDER;
            }

            center = center / counter as f32;

            if counter > 0 {
                if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, center) {
                    node.1.left = Val::Px(viewport_point.x);
                    node.1.top = Val::Px(viewport_point.y);

                    if node.2.0.0 == 1 {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::BrigadeSelection((node.2.0.0, brigade));
                            commands.entity(node.0).insert(UiImage::new(other.0.brigade_symbol_blufor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    } else {
                        if node.2.0.1.1 == false {
                            node.2.0.1.1 = true;
                            node.3.action = Actions::BrigadeSelection((node.2.0.0, brigade));
                            commands.entity(node.0).insert(UiImage::new(other.0.brigade_symbol_opfor.clone()));
                            commands.entity(node.0).insert(Visibility::Visible);
                        }
                    }
                }
                else if node.2.0.1.1 == true {
                    node.2.0.1.1 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
            else if node.2.0.1.1 == true {
                node.2.0.1.1 = false;
                commands.entity(node.0).insert(Visibility::Hidden);
            }
        } else if node.2.0.1.1 == true {
            node.2.0.1.1 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }

    for mut node in artillery_nodes_q.iter_mut() {//artillery selectors
        if camera.2.translation.y > 100. {
            if let Some(team_army) = army.0.get(&node.2.0.0) {
                if let Some(artillery_unit_army_reference) = team_army.artillery_units.0.get(&node.2.0.1) {
                    if let Some(artillery_unit_entity) = artillery_unit_army_reference.0.0 {
                        if let Ok(artillery_unit) = artillery_units_q.get(artillery_unit_entity) {
                            if artillery_unit.3 == Visibility::Hidden {
                                if node.2.0.2 == true {
                                    node.2.0.2 = false;
                                    commands.entity(node.0).insert(Visibility::Hidden);
                                }
                                continue;
                            }

                            if let Some(viewport_point) = camera.0.world_to_viewport(camera.1, artillery_unit.0.translation) {
                                node.1.left = Val::Px(viewport_point.x);
                                node.1.top = Val::Px(viewport_point.y);

                                if let Some(_selected) = artillery_unit.4 {
                                    if node.2.0.3 == Entity::PLACEHOLDER {
                                        let marker_size = other.1.button_size * 0.75;
                                        commands.entity(node.0).with_children(|parent| {
                                            node.2.0.3 = parent.spawn(
                                                NodeBundle {
                                                    style: Style {
                                                        position_type: PositionType::Absolute,
                                                        width: Val::Px(marker_size * 1.1),
                                                        height: Val::Px(marker_size * 1.1),
                                                        top: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                                        left: Val::Px((marker_size - marker_size * 1.1) * 0.5),
                                                        align_content: AlignContent::Center,
                                                        align_items: AlignItems::Center,
                                                        justify_content: JustifyContent::Center,
                                                        justify_items: JustifyItems::Center,
                                                        ..default()
                                                    },
                                                    background_color: Color::srgba(0.1, 1., 0.1, 0.25).into(),
                                                    ..default()
                                                }
                                            ).id();
                                        });
                                    }
                                } else if node.2.0.3 != Entity::PLACEHOLDER {
                                    commands.entity(node.2.0.3).despawn();

                                    node.2.0.3 = Entity::PLACEHOLDER;
                                }

                                if node.2.0.0 == 1 {
                                    if node.2.0.2 == false {
                                        node.2.0.2 = true;
                                        node.3.action = Actions::ArtilleryUnitSelection((node.2.0.0, node.2.0.1));
                                        commands.entity(node.0).insert(Visibility::Visible);

                                        commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_blufor.clone()));
                                    }

                                    if node.2.0.0 == player_data.team {
                                        if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                            let modifier: f32 = artillery_unit.2.supplies as f32 / artillery_unit.2.supplies_capacity as f32;
                                            let width = bar.1.original_width * modifier;
                                            
                                            bar.0.width = Val::Px(width);
                                        }
                                    } else {
                                        if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                            bar.0.width = Val::Px(0.);
                                        }
                                    }
                                } else {
                                    if node.2.0.2 == false {
                                        node.2.0.2 = true;
                                        node.3.action = Actions::ArtilleryUnitSelection((node.2.0.0, node.2.0.1));
                                        commands.entity(node.0).insert(Visibility::Visible);

                                        commands.entity(node.0).insert(UiImage::new(other.0.artillery_unit_symbol_opfor.clone()));
                                    }

                                    if node.2.0.0 == player_data.team {
                                        if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                            let modifier: f32 = artillery_unit.2.supplies as f32 / artillery_unit.2.supplies_capacity as f32;
                                            let width = bar.1.original_width * modifier;
                                            
                                            bar.0.width = Val::Px(width);
                                        }
                                    } else {
                                        if let Ok(mut bar) = supply_bars_q.get_mut(node.4.entity) {
                                            bar.0.width = Val::Px(0.);
                                        }
                                    }
                                }
                            }
                            else if node.2.0.2 == true {
                                node.2.0.2 = false;
                                commands.entity(node.0).insert(Visibility::Hidden);
                            }
                        }
                        else if node.2.0.2 == true {
                            node.2.0.2 = false;
                            commands.entity(node.0).insert(Visibility::Hidden);
                        }
                    } else if node.2.0.2 == true {
                        node.2.0.2 = false;
                        commands.entity(node.0).insert(Visibility::Hidden);
                    }
                } else if node.2.0.2 == true {
                    node.2.0.2 = false;
                    commands.entity(node.0).insert(Visibility::Hidden);
                }
            }
        }
        else if node.2.0.2 == true {
            node.2.0.2 = false;
            commands.entity(node.0).insert(Visibility::Hidden);
        }
    }
}

pub fn toggle_buildings_list_system(
    mut commands: Commands,
    resource_zones_q: Query<(Entity, &Transform, &ResourceZone), With<ResourceZone>>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    buildings_list: Res<BuildingsList>,
    mut building_placement_cache: ResMut<BuildingPlacementCache>,
    displayed_model_holders: Query<Entity, With<DisplayedModelHolder>>,
    mut deletion_states: ResMut<BuildingsDeletionStates>,
    selection_node: Query<Entity, With<SelectionBox>>,
    mut unit_selection: ResMut<IsUnitSelectionAllowed>,
    mut event_reader: EventReader<OpenBuildingsListEvent>,
    game_stage: Res<GameStage>,
){
    for _event in event_reader.read() {
        if matches!(game_stage.0, GameStages::GameStarted){
            deletion_states.is_blueprints_deletion_active = false;
            deletion_states.is_buildings_deletion_active = false;
            deletion_states.is_buildings_deletion_cancelation_active = false;

            unit_selection.0 = true;
            
            let selection_box = selection_node.single();
            commands.entity(selection_box).insert(BackgroundColor(Color::srgba(0., 1., 1., 0.1).into()));

            if ui_button_nodes.is_middle_bottom_node_visible {
                commands.entity(ui_button_nodes.middle_bottom_node).insert(Visibility::Hidden);
                ui_button_nodes.is_middle_bottom_node_visible = false;
                commands.entity(ui_button_nodes.middle_bottom_node_row).despawn_descendants();
                building_placement_cache.is_active = false;
                building_placement_cache.current_building = BuildingsBundles::None;
                for holder in displayed_model_holders.iter() {
                    commands.entity(holder).despawn();
                }
                building_placement_cache.current_building_y_adjustment = 0.;
                building_placement_cache.current_building_check_collider = Collider::ball(0.);

                for zone in resource_zones_q.iter(){
                    commands.entity(zone.0).remove::<CircleHolder>();
                }

                commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);
                for row in ui_button_nodes.left_bottom_node_rows.iter() {
                    commands.entity(*row).despawn_descendants();
                }
                ui_button_nodes.is_left_bottom_node_visible = false;
            }
            else{
                for zone in resource_zones_q.iter(){
                    commands.entity(zone.0).insert(CircleHolder(vec![
                        CircleData{
                            circle_center: zone.1.translation.xz(),
                            inner_radius: zone.2.zone_radius,
                            outer_radius: zone.2.zone_radius + 1.,
                            highlight_color: Vec4::new(0., 1., 0., 1.),
                        },
                    ]));
                }

                commands.entity(ui_button_nodes.middle_bottom_node_row).despawn_descendants();
                commands.entity(ui_button_nodes.middle_bottom_node).insert(Visibility::Visible);
                ui_button_nodes.is_middle_bottom_node_visible = true;

                commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);
                for row in ui_button_nodes.left_bottom_node_rows.iter() {
                    commands.entity(*row).despawn_descendants();
                }
                ui_button_nodes.is_left_bottom_node_visible = true;

                commands.entity(ui_button_nodes.left_bottom_node_rows[0]).with_children(|parent| {
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
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{action: Actions::ActivateBlueprintsDeletionMode})
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "DelBp".to_string(),
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default() 
                            },
                            ..default()
                        });
                    });

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
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{action: Actions::ActivateBuildingsDeletionMode})
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "DelBg".to_string(),
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default() 
                            },
                            ..default()
                        });
                    });

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
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{action: Actions::ActivateBuildingsDeletionCancelationMode})
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Cancel".to_string(),
                                    ..default()
                                }],
                                justify: JustifyText::Center,
                                ..default() 
                            },
                            ..default()
                        });
                    });
                });
    
                for building in buildings_list.0.iter() {
                    commands.entity(ui_button_nodes.middle_bottom_node_row).with_children(|parent| {
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
                            background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                            ..default()
                        }).insert(ButtonAction{action: Actions::BuildingToBuildSelected(
                            (building.1.clone(), building.2.clone(), building.3, building.4, building.0.clone(), building.5, building.6))}
                        )
                        .with_children(|button_parent| {
                            button_parent.spawn(TextBundle {
                                text: Text{
                                    sections: vec![TextSection {
                                        value: building.0.clone(),
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
            }
        }
    }
}

pub fn building_placement_activation_system(
    mut event_reader: EventReader<BuildingToBuildSelectedEvent>,
    mut building_placement_cache: ResMut<BuildingPlacementCache>,
    building_stage_cache: Res<BuildingStageCache>,
    displayed_model_holders: Query<Entity, With<DisplayedModelHolder>>,
    game_stage: Res<GameStage>,
    mut commands: Commands,
    player_data: Res<PlayerData>,
    mut materials: ResMut<Assets<StandardMaterial>>,
){
    for event in event_reader.read(){
        if matches!(game_stage.0, GameStages::BuildingsSetup) {
            if let Some(building) = building_stage_cache.buildings.get(&event.0.4) {
                if building.0 < 1 {
                    return;
                }
            }
        }

        building_placement_cache.is_active = true;
        building_placement_cache.current_building = event.0.0.clone();
        building_placement_cache.current_building_y_adjustment = event.0.2;
        building_placement_cache.current_building_check_collider = event.0.1.clone();
        building_placement_cache.needed_buildpower = event.0.3;
        building_placement_cache.name = event.0.4.clone();
        building_placement_cache.build_distance = event.0.5;
        building_placement_cache.resource_cost = event.0.6;

        for holder in displayed_model_holders.iter() {
            commands.entity(holder).despawn();
        }

        let color;
        if player_data.team == 1 {
            color = Color::srgba(0., 0., 1., 0.25);
        } else {
            color = Color::srgba(1., 0., 0., 0.25);
        }

        match &event.0.0 {
            BuildingsBundles::InfantryBarracks(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::VehicleFactory(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::LogisticHub(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::ResourceMiner(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder)
                .insert(ResourceZoneRestricted);
            },
            BuildingsBundles::Pillbox(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::WatchingTower(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::Autoturret(bundle) => {
                commands.spawn(PbrBundle{
                    mesh: bundle.model.mesh.clone(),
                    ..default()
                })
                .insert(NotShadowCaster)
                .insert(DisplayedModelHolder);
            },
            BuildingsBundles::None => {},
        }
    }
}

pub fn building_placement_handling_system(
    mut buildings_cache: (
        ResMut<BuildingPlacementCache>,
        ResMut<BuildingStageCache>,
        Res<BuildingsList>,
    ),
    materials: (
        Res<Assets<StandardMaterial>>,
        Res<InstancedMaterials>,
        Res<Assets<ExtendedMaterial<StandardMaterial, TeamMaterialExtension>>>,
    ),
    mut unactivated_blueprints: ResMut<UnactivatedBlueprints>,
    mut displayed_model_holders: Query<(Entity, &mut Transform, Option<&ResourceZoneRestricted>), (With<DisplayedModelHolder>, Without<ResourceZone>, Without<Terrain>)>,
    terrain_q: Query<Entity, With<Terrain>>,
    ui_resources: (
        Res<SelectionBounds>,
        Res<UiButtonNodes>,
    ),
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    button_inputs: (
        Res<ButtonInput<MouseButton>>,
        Res<ButtonInput<KeyCode>>,
    ),
    mut commands: Commands,
    mut resource_zones_q: Query<(&mut ResourceZone, &Transform), With<ResourceZone>>,
    game_stage: Res<GameStage>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    player_data: Res<PlayerData>,
    rapier_context: Res<RapierContext>,
){
    if buildings_cache.0.is_active {
        let terrain_entity = terrain_q.single();
        // let mut entities_to_ignore: Vec<Entity> = Vec::new();

        // for holder in displayed_model_holders.iter() {
        //     entities_to_ignore.push(holder.0);
        // }
        
        let mut cursor_on_plane_position = Vec3::ZERO;

        if let Some(cursor_ray) = **cursor_ray {
            let hits = raycast.cast_ray(cursor_ray, &RaycastSettings{
                filter: &move |entity| entity == terrain_entity,
                ..default()
            });

            if hits.len() > 0 {
                cursor_on_plane_position = hits[0].1.position();
            }
        }

        let angle = 45.0_f32.to_radians();

        let mut is_forbidden = false;

        if player_data.team == 1 {
            if !matches!(game_stage.0, GameStages::GameStarted) && cursor_on_plane_position.z > -ALLOWED_DISTANCE_FROM_BORDERS {
                is_forbidden = true;
            }
        } else {
            if !matches!(game_stage.0, GameStages::GameStarted) && cursor_on_plane_position.z < ALLOWED_DISTANCE_FROM_BORDERS {
                is_forbidden = true;
            }
        }

        let mut shape_position = cursor_on_plane_position;

        shape_position.y += 5.5;

        let intersections = rapier_context.intersection_with_shape(
            shape_position,
            Quat::from_rotation_y(angle),
            &buildings_cache.0.current_building_check_collider,
            QueryFilter::default(),
        );

        if intersections.is_some() {
            is_forbidden = true;
        }

        if cursor_on_plane_position.y > 5. {
            is_forbidden = true;
        }

        for mut holder in displayed_model_holders.iter_mut() {
            holder.1.translation = Vec3::new(
                cursor_on_plane_position.x,
                cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                cursor_on_plane_position.z,
            );
            holder.1.rotation = Quat::from_rotation_y(angle);

            if holder.2.is_some() {
                let mut is_inside_any_zone = false;
                for res_zone in resource_zones_q.iter() {
                    if cursor_on_plane_position.xz().distance(res_zone.1.translation.xz()) <= res_zone.0.zone_radius {
                        is_inside_any_zone = true;
                    }
                }

                if !is_inside_any_zone {
                    is_forbidden = true;
                }
            }

            if is_forbidden {
                commands.entity(holder.0).try_insert(ForbiddenBlueprint);
            } else {
                commands.entity(holder.0).remove::<ForbiddenBlueprint>();
            }
        }

        if button_inputs.0.just_pressed(MouseButton::Left) && !ui_resources.0.is_ui_hovered {
            if !is_forbidden {
                // let round_factor = 3.;
                // cursor_on_plane_position.x = ((cursor_on_plane_position.x / round_factor) as i32) as f32 * round_factor;
                // cursor_on_plane_position.z = ((cursor_on_plane_position.z / round_factor) as i32) as f32 * round_factor;

                if matches!(network_status.0, NetworkStatuses::Client) {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = client.connection_mut().send_message_on(channel_id, ClientMessage::BuildingPlacementRequest {
                            team: player_data.team,
                            name: buildings_cache.0.name.clone(),
                            position: cursor_on_plane_position,
                            angle: angle,
                            needed_buildpower: buildings_cache.0.needed_buildpower,
                        }){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }

                    let mut is_building_placed = false;
        
                    match &buildings_cache.0.current_building {
                        BuildingsBundles::InfantryBarracks(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::VehicleFactory(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::LogisticHub(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::ResourceMiner(bundle) => {
                            for mut zone in resource_zones_q.iter_mut() {
                                zone.0.current_miners.entry(player_data.team).or_insert_with(|| None);

                                let mut is_some = false;

                                if let Some(mut miner) = zone.0.current_miners.get_mut(&player_data.team) {
                                    if let Some(entity) = miner {
                                        if commands.get_entity(entity.0).is_none() {
                                            miner = &mut None;
                                        } else {
                                            is_some = true;
                                        }
                                    }
                                }
        
                                if !is_some && zone.1.translation.xz().distance(cursor_on_plane_position.xz()) <= zone.0.zone_radius {
                                    is_building_placed = true;
    
                                    break;
                                }
                            }
                        },
                        BuildingsBundles::Pillbox(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::WatchingTower(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::Autoturret(bundle) => {
                            is_building_placed = true;
                        },
                        BuildingsBundles::None => {},
                    }

                    if !button_inputs.1.pressed(KeyCode::ControlLeft) {
                        buildings_cache.0.is_active = false;
                        buildings_cache.0.current_building = BuildingsBundles::None;
                        for holder in displayed_model_holders.iter() {
                            commands.entity(holder.0).despawn();
                        }
                    }

                    if matches!(game_stage.0, GameStages::BuildingsSetup) && is_building_placed {
                        if let Some(building) = buildings_cache.1.buildings.get_mut(&buildings_cache.0.name) {
                            building.0 -= 1;

                            if building.0 < 1 {
                                buildings_cache.0.is_active = false;
                                buildings_cache.0.current_building = BuildingsBundles::None;
                                for holder in displayed_model_holders.iter() {
                                    commands.entity(holder.0).despawn();
                                }
                            }
                        }

                        commands.entity(ui_resources.1.middle_bottom_node_row).despawn_descendants();

                        for building in buildings_cache.2.0.iter() {
                            let mut count = "".to_string();
                            let mut color = Color::srgb(0., 1., 0.);
                            if let Some(building_cache) = buildings_cache.1.buildings.get_mut(&building.0) {
                                count = building_cache.0.to_string();
                                if building_cache.1 {
                                    color = Color::srgb(1., 0., 0.);
                                }

                                if building_cache.0 < 1 {
                                    color = Color::srgb(0., 1., 0.);
                                }
                            }
                            
                            commands.entity(ui_resources.1.middle_bottom_node_row).with_children(|parent| {
                                parent.spawn(ButtonBundle{
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(ui_resources.1.button_size - ui_resources.1.margin * 2.),
                                        height: Val::Px(ui_resources.1.button_size - ui_resources.1.margin * 2.),
                                        margin: UiRect {
                                            left: Val::Px(ui_resources.1.margin),
                                            right: Val::Px(ui_resources.1.margin),
                                            top: Val::Px(ui_resources.1.margin),
                                            bottom: Val::Px(ui_resources.1.margin),
                                        },
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                    ..default()
                                }).insert(ButtonAction{action: Actions::BuildingToBuildSelected(
                                    (building.1.clone(), building.2.clone(), building.3, building.4, building.0.clone(), building.5, building.6))}
                                )
                                .with_children(|button_parent| {
                                    button_parent.spawn(TextBundle {
                                        text: Text{
                                            sections: vec![TextSection {
                                                value: count,
                                                style: TextStyle{
                                                    color: color,
                                                    ..default()
                                                }
                                            }],
                                            justify: JustifyText::Center,
                                            ..default() 
                                        },
                                        ..default()
                                    });
                                });
                            });
                        }
                    }
                } else {
                    let mut new_building_entity = Entity::PLACEHOLDER;

                    let color;
                    if player_data.team == 1 {
                        color = Vec4::new(0., 0., 1., 1.);
                    } else {
                        color = Vec4::new(1., 0., 0., 1.);
                    }
        
                    match &buildings_cache.0.current_building {
                        BuildingsBundles::InfantryBarracks(bundle) => {
                            let material = materials.1.blue_transparent.clone();
    
                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::VehicleFactory(bundle) => {
                            let material = materials.1.blue_transparent.clone();
    
                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::LogisticHub(bundle) => {
                            let material = materials.1.blue_transparent.clone();

                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::ResourceMiner(bundle) => {
                            for mut zone in resource_zones_q.iter_mut() {
                                zone.0.current_miners.entry(player_data.team).or_insert_with(|| None);

                                let mut is_some = false;

                                if let Some(mut miner) = zone.0.current_miners.get_mut(&player_data.team) {
                                    if let Some(entity) = miner {
                                        if commands.get_entity(entity.0).is_none() {
                                            miner = &mut None;
                                        } else {
                                            is_some = true;
                                        }
                                    }
                                }
        
                                if !is_some && zone.1.translation.xz().distance(cursor_on_plane_position.xz()) <= zone.0.zone_radius {
                                    let material = materials.1.blue_transparent.clone();
            
                                    new_building_entity = commands.spawn(MaterialMeshBundle{
                                        mesh: bundle.model.mesh.clone(),
                                        material: material.clone(),
                                        transform: Transform::from_translation(Vec3::new(
                                            cursor_on_plane_position.x,
                                            cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                            cursor_on_plane_position.z,
                                        )).with_rotation(Quat::from_rotation_y(angle)),
                                        ..default()
                                    }).insert(BuildingBlueprint{
                                        team: player_data.team,
                                        building_bundle: buildings_cache.0.current_building.clone(),
                                        build_power_remaining: buildings_cache.0.needed_buildpower,
                                        name: buildings_cache.0.name.clone(),
                                        build_distance: buildings_cache.0.build_distance,
                                        resource_cost: buildings_cache.0.resource_cost,
                                    })
                                    .insert(NotShadowCaster)
                                    .id();

                                    if let Some(miner) = zone.0.current_miners.get_mut(&player_data.team) {
                                        *miner = Some((new_building_entity, 0));
                                    }
    
                                    break;
                                }
                            }
                        },
                        BuildingsBundles::Pillbox(bundle) => {
                            let material = materials.1.blue_transparent.clone();

                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::WatchingTower(bundle) => {
                            let material = materials.1.blue_transparent.clone();

                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::Autoturret(bundle) => {
                            let material = materials.1.blue_transparent.clone();

                            new_building_entity = commands.spawn(MaterialMeshBundle{
                                mesh: bundle.model.mesh.clone(),
                                material: material.clone(),
                                transform: Transform::from_translation(Vec3::new(
                                    cursor_on_plane_position.x,
                                    cursor_on_plane_position.y + buildings_cache.0.current_building_y_adjustment,
                                    cursor_on_plane_position.z,
                                )).with_rotation(Quat::from_rotation_y(angle)),
                                ..default()
                            }).insert(BuildingBlueprint{
                                team: player_data.team,
                                building_bundle: buildings_cache.0.current_building.clone(),
                                build_power_remaining: buildings_cache.0.needed_buildpower,
                                name: buildings_cache.0.name.clone(),
                                build_distance: buildings_cache.0.build_distance,
                                resource_cost: buildings_cache.0.resource_cost,
                            })
                            .insert(NotShadowCaster)
                            .id();
                        },
                        BuildingsBundles::None => {},
                    }
        
                    if new_building_entity != Entity::PLACEHOLDER {
                        if let GameStages::GameStarted = game_stage.0 {
                            unactivated_blueprints.0.entry(player_data.team).or_insert_with(HashMap::new)
                            .insert(new_building_entity, (cursor_on_plane_position, Entity::PLACEHOLDER, buildings_cache.0.build_distance));
                        }
                    }

                    if !button_inputs.1.pressed(KeyCode::ControlLeft) {
                        buildings_cache.0.is_active = false;
                        buildings_cache.0.current_building = BuildingsBundles::None;
                        for holder in displayed_model_holders.iter() {
                            commands.entity(holder.0).despawn();
                        }
                    }

                    if matches!(game_stage.0, GameStages::BuildingsSetup) && new_building_entity != Entity::PLACEHOLDER {
                        if let Some(building) = buildings_cache.1.buildings.get_mut(&buildings_cache.0.name) {
                            building.0 -= 1;

                            if building.0 < 1 {
                                buildings_cache.0.is_active = false;
                                buildings_cache.0.current_building = BuildingsBundles::None;
                                for holder in displayed_model_holders.iter() {
                                    commands.entity(holder.0).despawn();
                                }
                            }
                        }

                        commands.entity(ui_resources.1.middle_bottom_node_row).despawn_descendants();

                        for building in buildings_cache.2.0.iter() {
                            let mut count = "".to_string();
                            let mut color = Color::srgb(0., 1., 0.);
                            if let Some(building_cache) = buildings_cache.1.buildings.get_mut(&building.0) {
                                count = building_cache.0.to_string();
                                if building_cache.1 {
                                    color = Color::srgb(1., 0., 0.);
                                }

                                if building_cache.0 < 1 {
                                    color = Color::srgb(0., 1., 0.);
                                }
                            }
                            
                            commands.entity(ui_resources.1.middle_bottom_node_row).with_children(|parent| {
                                parent.spawn(ButtonBundle{
                                    style: Style {
                                        position_type: PositionType::Relative,
                                        width: Val::Px(ui_resources.1.button_size - ui_resources.1.margin * 2.),
                                        height: Val::Px(ui_resources.1.button_size - ui_resources.1.margin * 2.),
                                        margin: UiRect {
                                            left: Val::Px(ui_resources.1.margin),
                                            right: Val::Px(ui_resources.1.margin),
                                            top: Val::Px(ui_resources.1.margin),
                                            bottom: Val::Px(ui_resources.1.margin),
                                        },
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                    ..default()
                                }).insert(ButtonAction{action: Actions::BuildingToBuildSelected(
                                    (building.1.clone(), building.2.clone(), building.3, building.4, building.0.clone(), building.5, building.6))}
                                )
                                .with_children(|button_parent| {
                                    button_parent.spawn(TextBundle {
                                        text: Text{
                                            sections: vec![TextSection {
                                                value: count,
                                                style: TextStyle{
                                                    color: color,
                                                    ..default()
                                                }
                                            }],
                                            justify: JustifyText::Center,
                                            ..default() 
                                        },
                                        ..default()
                                    });
                                });
                            });
                        }
                    }
                }
            }
        } else if button_inputs.0.just_pressed(MouseButton::Right) {
            buildings_cache.0.is_active = false;
            buildings_cache.0.name = "".to_string();
            buildings_cache.0.build_distance = 0.;
            buildings_cache.0.resource_cost = 0;

            for holder in displayed_model_holders.iter() {
                commands.entity(holder.0).despawn();
            }
        }
    }
}

pub fn toggle_artillety_management_node (
    selected_artillery_unit_q: Query<Entity, (With<ArtilleryUnit>, With<SelectedUnit>)>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mut commands: Commands,
    mut state: Local<bool>,
){
    if !selected_artillery_unit_q.is_empty() && !ui_button_nodes.is_left_bottom_node_visible && !ui_button_nodes.is_middle_bottom_node_visible {
        *state = true;

        ui_button_nodes.is_left_bottom_node_visible = true;
        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);

        commands.entity(ui_button_nodes.left_bottom_node_rows[0]).with_children(|parent| {
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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::ToggleArtilleryDesignation})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Ast".to_string(),
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default() 
                    },
                    ..default()
                });
            });
        });

        commands.entity(ui_button_nodes.left_bottom_node_rows[0]).with_children(|parent| {
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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::CancelArtilleryTargets})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Asp".to_string(),
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default() 
                    },
                    ..default()
                });
            });
        });
    } else if selected_artillery_unit_q.is_empty() && ui_button_nodes.is_left_bottom_node_visible && !ui_button_nodes.is_middle_bottom_node_visible && *state {
        *state = false;

        ui_button_nodes.is_left_bottom_node_visible = false;
        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);

        for node_row in ui_button_nodes.left_bottom_node_rows.iter() {
            commands.entity(*node_row).despawn_descendants();
        }
    }
}

pub fn settlements_stage_ui_activation(
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mut commands: Commands,
){
    commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();
    commands.entity(ui_button_nodes.middle_upper_node).insert(Visibility::Visible);
    ui_button_nodes.is_middle_upper_node_visible = true;

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
                        value: format!("Cities to place left: {0}", CITIES_COUNT),
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

pub fn building_stage_ui_activation(
    mut event_reader: EventReader<AllSettlementsPlaced>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    buildings_list: Res<BuildingsList>,
    building_stage_cache: Res<BuildingStageCache>,
    mut commands: Commands,
){
    for _event in event_reader.read() {
        commands.entity(ui_button_nodes.middle_bottom_node_row).despawn_descendants();
        commands.entity(ui_button_nodes.middle_bottom_node).insert(Visibility::Visible);
        ui_button_nodes.is_middle_bottom_node_visible = true;

        for building in buildings_list.0.iter() {
            let mut count = "".to_string();
            let mut color = Color::srgb(0., 1., 0.);
            if let Some(building_cache) = building_stage_cache.buildings.get(&building.0) {
                count = building_cache.0.to_string();

                if building_cache.1 {
                    color = Color::srgb(1., 0., 0.);
                }
            }
            
            commands.entity(ui_button_nodes.middle_bottom_node_row).with_children(|parent| {
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
                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                    ..default()
                }).insert(ButtonAction{action: Actions::BuildingToBuildSelected(
                    (building.1.clone(), building.2.clone(), building.3, building.4, building.0.clone(), building.5, building.6))}
                )
                .with_children(|button_parent| {
                    button_parent.spawn(TextBundle {
                        text: Text{
                            sections: vec![TextSection {
                                value: count,
                                style: TextStyle{
                                    color: color,
                                    ..default()
                                },
                            }],
                            justify: JustifyText::Center,
                            ..default() 
                        },
                        ..default()
                    });
                });
            });
        }

        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);
        for row in ui_button_nodes.left_bottom_node_rows.iter() {
            commands.entity(*row).despawn_descendants();
        }
        ui_button_nodes.is_left_bottom_node_visible = true;

        commands.entity(ui_button_nodes.left_bottom_node_rows[0]).with_children(|parent| {
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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::ActivateBlueprintsDeletionMode})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "DelBp".to_string(),
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default() 
                    },
                    ..default()
                });
            });

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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::ActivateBuildingsDeletionMode})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "DelBg".to_string(),
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default() 
                    },
                    ..default()
                });
            });

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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::ActivateBuildingsDeletionCancelationMode})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Cancel".to_string(),
                            ..default()
                        }],
                        justify: JustifyText::Center,
                        ..default() 
                    },
                    ..default()
                });
            });
        });

        commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();
        commands.entity(ui_button_nodes.middle_upper_node).insert(Visibility::Visible);
        ui_button_nodes.is_middle_upper_node_visible = true;

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
            .insert(ButtonAction{
                action: Actions::CompleteConstruction,
            })
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Complete the construction".to_string(),
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
}

pub fn army_setup_stage_ui_activation(
    mut event_reader: EventReader<CompleteConstruction>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mut army_settings_nodes: ResMut<ArmySettingsNodes>,
    resource_zones_q: Query<Entity, With<ResourceZone>>,
    building_stage_cache: Res<BuildingStageCache>,
    mut commands: Commands,
    mut game_stage: ResMut<GameStage>,
){
    for _event in event_reader.read() {
        for building in building_stage_cache.buildings.iter() {
            if building.1.0 > 0 && building.1.1 {
                return;
            }
        }

        game_stage.0 = GameStages::ArmySetup;

        commands.entity(ui_button_nodes.middle_bottom_node_row).despawn_descendants();
        commands.entity(ui_button_nodes.middle_bottom_node).insert(Visibility::Hidden);
        ui_button_nodes.is_middle_bottom_node_visible = false;

        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);
        for row in ui_button_nodes.left_bottom_node_rows.iter() {
            commands.entity(*row).despawn_descendants();
        }
        ui_button_nodes.is_left_bottom_node_visible = false;

        commands.entity(ui_button_nodes.middle_upper_node_row).despawn_descendants();
        commands.entity(ui_button_nodes.middle_upper_node).insert(Visibility::Hidden);
        ui_button_nodes.is_middle_upper_node_visible = false;

        commands.entity(army_settings_nodes.land_army_settings_node).insert(Visibility::Visible);
        army_settings_nodes.is_land_army_settings_visible = true;

        for zone in resource_zones_q.iter(){
            commands.entity(zone).remove::<CircleHolder>();
        }
    }
}

pub fn tactical_symbols_dropdown_menu_system (
    mut event_reader: EventReader<OpenTacticalSymbolsLevels>,
    mut commands: Commands,
    ui_nodes: Res<UiButtonNodes>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    button_interaction_q: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut press_count: Local<LimitedNumber<0, 2>>,
){
    for _event in event_reader.read() {
        if press_count.next() {
            commands.entity(ui_nodes.symbol_level_dropdown_list).despawn_descendants();
        } else {
            commands.entity(ui_nodes.symbol_level_dropdown_list).despawn_descendants();

            commands.entity(ui_nodes.symbol_level_dropdown_list).with_children(|parent| {
                parent.spawn(NodeBundle{
                    style: Style {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(ui_nodes.button_size / 2.),
                        width: Val::Px(ui_nodes.button_size),
                        height: Val::Px(ui_nodes.button_size / 4. * 9.),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: Color::srgba(0.1, 0.1, 0.1, 0.5).into(),
                    ..default()
                })
                .with_children(|parent| {
                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(1),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Squads".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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

                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(2),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Platoons".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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

                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(3),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Companies".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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

                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(4),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Battalions".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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

                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(5),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Regiments".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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

                    parent.spawn(ButtonBundle{
                        style: Style {
                            position_type: PositionType::Relative,
                            width: Val::Px(ui_nodes.button_size - ui_nodes.margin * 2.),
                            height: Val::Px((ui_nodes.button_size - ui_nodes.margin * 2.) / 4.),
                            margin: UiRect {
                                left: Val::Px(ui_nodes.margin),
                                right: Val::Px(ui_nodes.margin),
                                top: Val::Px(ui_nodes.margin),
                                bottom: Val::Px(ui_nodes.margin),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                        ..default()
                    }).insert(ButtonAction{
                        action: Actions::ChangeTacticalSymbolsLevel(6),
                    })
                    .with_children(|button_parent| {
                        button_parent.spawn(TextBundle {
                            text: Text{
                                sections: vec![TextSection {
                                    value: "Army".to_string(),
                                    style: TextStyle {
                                        font_size: 10.,
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
            });
        }
    }

    if mouse_buttons.any_just_pressed([MouseButton::Left, MouseButton::Right]) {
        let mut is_any_butten_pressed = false;
        for interaction in button_interaction_q.iter() {
            if matches!(interaction, Interaction::Pressed) {
                is_any_butten_pressed = true;
            }
        }

        if !is_any_butten_pressed {
            commands.entity(ui_nodes.symbol_level_dropdown_list).despawn_descendants();
            press_count.set_value(0);
        }
    }
}

#[derive(Resource)]
pub struct DisplayedTacicalSymbolsLevel(pub i32);

pub fn tactical_symbols_level_choose_system (
    mut event_reader: EventReader<ChangeTacticalSymbolsLevel>,
    ui_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
    mut displayed_level: ResMut<DisplayedTacicalSymbolsLevel>,
    mut event_writer: EventWriter<OpenTacticalSymbolsLevels>,
){
    for event in event_reader.read() {
        event_writer.send(OpenTacticalSymbolsLevels);

        displayed_level.0 = event.0;

        match event.0 {
            1 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Squads",
                    TextStyle {
                        ..default()
                    }),
                );
            },
            2 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Platoons",
                    TextStyle {
                        ..default()
                    }),
                );
            },
            3 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Companies",
                    TextStyle {
                        ..default()
                    }),
                );
            },
            4 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Battalions",
                    TextStyle {
                        ..default()
                    }),
                );
            }
            5 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Regiments",
                    TextStyle {
                        ..default()
                    }),
                );
            },
            6 => {
                commands.entity(ui_nodes.symbol_level_dropdown_list).insert(Text::from_section(
                    "Army",
                    TextStyle {
                        ..default()
                    }),
                );
            }
            _ => {},
        }
    }
}

#[derive(Event)]
pub struct StartSingleplayerEvent;

#[derive(Event)]
pub struct HostNewGameEvent;

#[derive(Event)]
pub struct ConnectToHostedGameEvent;

pub fn main_menu_ui_system (
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut ip_buffer: ResMut<InsertedConnectionData>,
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut event_writer: (
        EventWriter<StartSingleplayerEvent>,
        EventWriter<HostNewGameEvent>,
        EventWriter<ConnectToHostedGameEvent>,
    ),
    mut exit_events: EventWriter<AppExit>,
    mut network_status: ResMut<NetworkStatus>,
    mut current_menu_page_id: Local<i32>,
    mut initial_delay: Local<u128>,
    time: Res<Time>,
){
    if *initial_delay < 3000 {
        *initial_delay += time.delta().as_millis();
        return;
    }

    let ctx = contexts.ctx_mut();
    let window = windows_q.single();
    let window_width = window.physical_width() as f32;
    let window_height = window.physical_height() as f32;

    match *current_menu_page_id {
        0 => {
            let main_menu_node_width = window_width * 0.8;
            let main_menu_node_height = window_height * 0.8;

            let x = (window_width - main_menu_node_width) / 2.;
            let y = (window_height - main_menu_node_height) / 4.;
            
            egui::Window::new("Main menu")
            .default_pos(egui::Pos2::new(x, y))
            .default_size(egui::Vec2::new(main_menu_node_width, main_menu_node_height))
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .show(&ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(main_menu_node_height * 0.05);

                    if ui.add(
                        egui::Button::new(
                                egui::RichText::new("Singleplayer")
                                .size(main_menu_node_height * 0.1)
                                .color(egui::Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(0, 0, 0))
                            .stroke(Stroke{
                                width: 0.1,
                                color: Color32::from_rgb(255, 255, 255),
                            })
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        next_state.set(GameState::Singleplayer);
                        event_writer.0.send(StartSingleplayerEvent);
                        network_status.0 = NetworkStatuses::SinglePlayer;
                    }

                    ui.add_space(main_menu_node_height * 0.1);

                    if ui.add(
                        egui::Button::new(
                                egui::RichText::new("Multiplayer")
                                .size(main_menu_node_height * 0.1)
                                .color(egui::Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(0, 0, 0))
                            .stroke(Stroke{
                                width: 0.1,
                                color: Color32::from_rgb(255, 255, 255),
                            })
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        *current_menu_page_id = 1;
                    }

                    ui.add_space(main_menu_node_height * 0.1);

                    if ui.add(
                        egui::Button::new(
                                egui::RichText::new("Settings")
                                .size(main_menu_node_height * 0.1)
                                .color(egui::Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(0, 0, 0))
                            .stroke(Stroke{
                                width: 0.1,
                                color: Color32::from_rgb(255, 255, 255),
                            })
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                    }

                    ui.add_space(main_menu_node_height * 0.1);

                    if ui.add(
                        egui::Button::new(
                                egui::RichText::new("Close the game")
                                .size(main_menu_node_height * 0.1)
                                .color(egui::Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(0, 0, 0))
                            .stroke(Stroke{
                                width: 0.1,
                                color: Color32::from_rgb(255, 255, 255),
                            })
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        exit_events.send(AppExit::Success);
                    }

                    ui.add_space(main_menu_node_height * 0.1);
                });
            });
        }
        1 => {
            let main_menu_node_width = window_width * 0.8;
            let main_menu_node_height = window_height * 0.8;

            let x = (window_width - main_menu_node_width) / 2.;
            let y = (window_height - main_menu_node_height) / 4.;

            egui::Window::new("Multiplayer")
            .default_pos(egui::Pos2::new(x, y))
            .default_size(egui::Vec2::new(main_menu_node_width, main_menu_node_height))
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .show(&ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(main_menu_node_height * 0.01);

                    ui.label(
                        egui::RichText::new("Nickname")
                        .size(main_menu_node_height * 0.1)
                        .color(egui::Color32::WHITE)
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut ip_buffer.username)
                            .desired_width(main_menu_node_width * 0.8)
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                            .font(FontId { size: main_menu_node_height * 0.1, family: egui::FontFamily::Proportional })
                            .text_color(egui::Color32::WHITE)
                    );

                    ui.add_space(main_menu_node_height * 0.01);

                    if ui.add(
                        egui::Button::new(
                        egui::RichText::new("Host")
                        .size(main_menu_node_height * 0.1)
                        .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        next_state.set(GameState::LobbyAsServer);
                        event_writer.1.send(HostNewGameEvent);
                        network_status.0 = NetworkStatuses::Host;
                        
                        *current_menu_page_id = 0;
                    }

                    ui.add_space(main_menu_node_height * 0.01);

                    ui.label(
                        egui::RichText::new("IP:Port")
                        .size(main_menu_node_height * 0.1)
                        .color(egui::Color32::WHITE)
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut ip_buffer.ip)
                            .desired_width(main_menu_node_width * 0.8)
                            .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                            .font(FontId { size: main_menu_node_height * 0.1, family: egui::FontFamily::Proportional })
                            .text_color(egui::Color32::WHITE)
                    );

                    ui.add_space(main_menu_node_height * 0.01);

                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new("Connect")
                            .size(main_menu_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        next_state.set(GameState::LobbyAsClient);
                        event_writer.2.send(ConnectToHostedGameEvent);
                        network_status.0 = NetworkStatuses::Client;

                        *current_menu_page_id = 0;
                    }

                    ui.add_space(main_menu_node_height * 0.01);

                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new("Back")
                            .size(main_menu_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                    ).clicked() {
                        *current_menu_page_id = 0;
                    }

                    ui.add_space(main_menu_node_height * 0.01);
                });
            });
        }
        _ => {}
    }
}

pub fn game_end_ui_system (
    captured_settlements_record: Res<Settlements>,
    player_data: Res<PlayerData>,
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut ip_buffer: ResMut<InsertedConnectionData>,
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
){
    let ctx = contexts.ctx_mut();
    let window = windows_q.single();
    let window_width = window.physical_width() as f32;
    let window_height = window.physical_height() as f32;

    let main_menu_node_width = window_width * 0.8;
    let main_menu_node_height = window_height * 0.5;

    let x = (window_width - main_menu_node_width) / 2.;
    let y = (window_height - main_menu_node_height) / 2.;

    egui::Window::new("Game ended")
        .default_pos(egui::Pos2::new(x, y))
        .default_size(egui::Vec2::new(main_menu_node_width, main_menu_node_height))
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(main_menu_node_height * 0.2);

                let mut text = "";
                if let Some(team_settlements) = captured_settlements_record.0.get(&player_data.team) {
                    if *team_settlements == 0 {
                        text = "You've lost";
                    } else {
                        text = "You've won";
                    }
                }

                ui.label(
                    egui::RichText::new(text)
                    .size(main_menu_node_height * 0.1)
                    .color(egui::Color32::WHITE)
                );

                ui.add_space(main_menu_node_height * 0.05);

                if ui.add(
                    egui::Button::new(
                            egui::RichText::new("Ok")
                            .size(main_menu_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                ).clicked() {
                    ip_buffer.username = "".to_string();
                    ip_buffer.ip = "".to_string();

                    let mut settlements_record: HashMap<i32, i32> = HashMap::new();

                    settlements_record.insert(1, VILLAGES_COUNT + CITIES_COUNT);
                    settlements_record.insert(2, VILLAGES_COUNT + CITIES_COUNT);

                    commands.insert_resource(Settlements(settlements_record));
                    
                    commands.insert_resource(PlayerData{
                        team: 1,
                        is_all_settlements_placed: false,
                        is_ready_to_start: false,
                    });
                    commands.insert_resource(components::unit::TargetPosition{
                        position: Vec3::new(0., 0., 0.),
                    });
                    commands.insert_resource(components::unit::SelectedUnits{
                        platoons: HashMap::new(),
                    });
                    commands.insert_resource(components::camera::SelectionBounds{
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
                    });
                    commands.insert_resource(components::camera::Formation{
                        points: Vec::new(),
                        is_formation_active: false,
                    });
                    commands.insert_resource(components::unit::UnitsTileMap{
                        tiles: HashMap::new(),
                    });
                    commands.insert_resource(components::camera::TimerResource(Timer::from_seconds(0.5, TimerMode::Repeating)));
                    commands.insert_resource(components::unit::AsyncPathfindingTasks{
                        tasks: Vec::new(),
                    });
                    commands.insert_resource(components::building::SelectedBuildings{
                        buildings: Vec::new(),
                    });
                    commands.insert_resource(components::ui_manager::UiButtonNodes {
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
                    });
                    commands.insert_resource(components::unit::Armies(HashMap::new()));
                    commands.insert_resource(ArmySettingsNodes {
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
                    });
                    commands.insert_resource(Specializations{
                        regular: Vec::new(),
                        shock: Vec::new(),
                        armored: Vec::new(),
                    });
                    commands.insert_resource(ProductionState{
                        is_allowed: HashMap::new(),
                    });
                    commands.insert_resource(ProductionQueue(HashMap::new()));
                    commands.insert_resource(BuildingsList(
                        Vec::new(),
                    ));
                    commands.insert_resource(BuildingPlacementCache {
                        is_active: false,
                        current_building: BuildingsBundles::None,
                        current_building_y_adjustment: 0.,
                        current_building_check_collider: Collider::ball(0.),
                        needed_buildpower: 0,
                        name: "".to_string(),
                        build_distance: 0.,
                        resource_cost: 0,
                    });
                    commands.insert_resource(UnactivatedBlueprints(HashMap::new()));
                    commands.insert_resource(GameStage(GameStages::SettlementsSetup));
                    commands.insert_resource(SettlementsLeft(Vec::new()));
                    commands.insert_resource(IsArtilleryDesignationActive(false));
                    commands.insert_resource(IsUnitDeselectionAllowed(true));
                    commands.insert_resource(AsyncTaskPools{
                        manual_pathfinding_pool: TaskPool::new(),
                        logistic_pathfinding_pool: TaskPool::new(),
                        extra_pathfinding_pool: TaskPool::new(),
                    });
                    commands.insert_resource(NetworkStatus(NetworkStatuses::SinglePlayer));
                    commands.insert_resource(InsertedConnectionData{
                        ip: "".to_string(),
                        username: "".to_string(),
                    });
                    commands.insert_resource(ClientList(HashMap::new()));
                    commands.insert_resource(PlayerList(HashMap::new()));
                    commands.insert_resource(EntityMaps{
                        server_to_client: HashMap::new(),
                        client_to_server: HashMap::new(),
                    });
                    commands.insert_resource(ProducableUnits{
                        barrack_producables: HashMap::new(),
                        factory_producables: HashMap::new(),
                    });
                    commands.insert_resource(UnspecifiedEntitiesToMove(Vec::new()));
                    commands.insert_resource(UnitsToDamage(Vec::new()));
                    commands.insert_resource(UnitsToInsertPath(Vec::new()));
                    commands.insert_resource(InstancedMaterials{
                        team_materials: HashMap::new(),
                        blue_solid: Handle::default(),
                        red_solid: Handle::default(),
                        blue_transparent: Handle::default(),
                        red_transparent: Handle::default(),
                        wreck_material: Handle::default(),
                        road_material: Handle::default(),
                    });
                    commands.insert_resource(DisplayedTacicalSymbolsLevel(1));
                    commands.insert_resource(IsUnitSelectionAllowed(true));
                    commands.insert_resource(BuildingsDeletionStates{
                        is_blueprints_deletion_active: false,
                        is_buildings_deletion_active: false,
                        is_buildings_deletion_cancelation_active: false,
                    });
                    commands.insert_resource(UiBlocker{
                        is_bottom_left_node_blocked: false,
                        is_bottom_middle_node_blocked: false,
                    });
                    commands.insert_resource(BuildingStageCache{
                        buildings: HashMap::new(),
                    });
                    commands.insert_resource(InstancedAnimations{
                        running_animations: HashMap::new(),
                    });
                    commands.insert_resource(RemainsCount(0));

                    next_state.set(GameState::MainMenu);
                }

                ui.add_space(main_menu_node_height * 0.2);
            });
        });
}

pub fn esc_menu_ui_system (
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut ip_buffer: ResMut<InsertedConnectionData>,
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: Local<bool>,
){
    if keys.just_pressed(KeyCode::Escape) {
        *state = !*state;
    }

    if *state {
        let ctx = contexts.ctx_mut();
        let window = windows_q.single();
        let window_width = window.physical_width() as f32;
        let window_height = window.physical_height() as f32;

        let main_menu_node_width = window_width * 0.8;
        let main_menu_node_height = window_height * 0.5;

        let x = (window_width - main_menu_node_width) / 2.;
        let y = (window_height - main_menu_node_height) / 2.;

        egui::Window::new("ESC menu")
        .default_pos(egui::Pos2::new(x, y))
        .default_size(egui::Vec2::new(main_menu_node_width, main_menu_node_height))
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(main_menu_node_height * 0.2);

                if ui.add(
                    egui::Button::new(
                            egui::RichText::new("Exit to main menu")
                            .size(main_menu_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(main_menu_node_width * 0.8, main_menu_node_height * 0.15))
                ).clicked() {
                    *state = false;

                    ip_buffer.username = "".to_string();
                    ip_buffer.ip = "".to_string();

                    let mut settlements_record: HashMap<i32, i32> = HashMap::new();

                    settlements_record.insert(1, VILLAGES_COUNT + CITIES_COUNT);
                    settlements_record.insert(2, VILLAGES_COUNT + CITIES_COUNT);

                    commands.insert_resource(Settlements(settlements_record));
                    
                    commands.insert_resource(PlayerData{
                        team: 1,
                        is_all_settlements_placed: false,
                        is_ready_to_start: false,
                    });
                    commands.insert_resource(components::unit::TargetPosition{
                        position: Vec3::new(0., 0., 0.),
                    });
                    commands.insert_resource(components::unit::SelectedUnits{
                        platoons: HashMap::new(),
                    });
                    commands.insert_resource(components::camera::SelectionBounds{
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
                    });
                    commands.insert_resource(components::camera::Formation{
                        points: Vec::new(),
                        is_formation_active: false,
                    });
                    commands.insert_resource(components::unit::UnitsTileMap{
                        tiles: HashMap::new(),
                    });
                    commands.insert_resource(components::camera::TimerResource(Timer::from_seconds(0.5, TimerMode::Repeating)));
                    commands.insert_resource(components::unit::AsyncPathfindingTasks{
                        tasks: Vec::new(),
                    });
                    commands.insert_resource(components::building::SelectedBuildings{
                        buildings: Vec::new(),
                    });
                    commands.insert_resource(components::ui_manager::UiButtonNodes {
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
                    });
                    commands.insert_resource(components::unit::Armies(HashMap::new()));
                    commands.insert_resource(ArmySettingsNodes {
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
                    });
                    commands.insert_resource(Specializations{
                        regular: Vec::new(),
                        shock: Vec::new(),
                        armored: Vec::new(),
                    });
                    commands.insert_resource(ProductionState{
                        is_allowed: HashMap::new(),
                    });
                    commands.insert_resource(ProductionQueue(HashMap::new()));
                    commands.insert_resource(BuildingsList(
                        Vec::new(),
                    ));
                    commands.insert_resource(BuildingPlacementCache {
                        is_active: false,
                        current_building: BuildingsBundles::None,
                        current_building_y_adjustment: 0.,
                        current_building_check_collider: Collider::ball(0.),
                        needed_buildpower: 0,
                        name: "".to_string(),
                        build_distance: 0.,
                        resource_cost: 0,
                    });
                    commands.insert_resource(UnactivatedBlueprints(HashMap::new()));
                    commands.insert_resource(GameStage(GameStages::SettlementsSetup));
                    commands.insert_resource(SettlementsLeft(Vec::new()));
                    commands.insert_resource(IsArtilleryDesignationActive(false));
                    commands.insert_resource(IsUnitDeselectionAllowed(true));
                    commands.insert_resource(AsyncTaskPools{
                        manual_pathfinding_pool: TaskPool::new(),
                        logistic_pathfinding_pool: TaskPool::new(),
                        extra_pathfinding_pool: TaskPool::new(),
                    });
                    commands.insert_resource(NetworkStatus(NetworkStatuses::SinglePlayer));
                    commands.insert_resource(InsertedConnectionData{
                        ip: "".to_string(),
                        username: "".to_string(),
                    });
                    commands.insert_resource(ClientList(HashMap::new()));
                    commands.insert_resource(PlayerList(HashMap::new()));
                    commands.insert_resource(EntityMaps{
                        server_to_client: HashMap::new(),
                        client_to_server: HashMap::new(),
                    });
                    commands.insert_resource(ProducableUnits{
                        barrack_producables: HashMap::new(),
                        factory_producables: HashMap::new(),
                    });
                    commands.insert_resource(UnspecifiedEntitiesToMove(Vec::new()));
                    commands.insert_resource(UnitsToDamage(Vec::new()));
                    commands.insert_resource(UnitsToInsertPath(Vec::new()));
                    commands.insert_resource(InstancedMaterials{
                        team_materials: HashMap::new(),
                        blue_solid: Handle::default(),
                        red_solid: Handle::default(),
                        blue_transparent: Handle::default(),
                        red_transparent: Handle::default(),
                        wreck_material: Handle::default(),
                        road_material: Handle::default(),
                    });
                    commands.insert_resource(DisplayedTacicalSymbolsLevel(1));
                    commands.insert_resource(IsUnitSelectionAllowed(true));
                    commands.insert_resource(BuildingsDeletionStates{
                        is_blueprints_deletion_active: false,
                        is_buildings_deletion_active: false,
                        is_buildings_deletion_cancelation_active: false,
                    });
                    commands.insert_resource(UiBlocker{
                        is_bottom_left_node_blocked: false,
                        is_bottom_middle_node_blocked: false,
                    });
                    commands.insert_resource(BuildingStageCache{
                        buildings: HashMap::new(),
                    });
                    commands.insert_resource(InstancedAnimations{
                        running_animations: HashMap::new(),
                    });
                    commands.insert_resource(RemainsCount(0));

                    next_state.set(GameState::MainMenu);
                }

                ui.add_space(main_menu_node_height * 0.2);
            });
        });
    }
}

pub fn show_lobby_as_server(
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
    players: ResMut<PlayerList>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
    mut next_state: ResMut<NextState<GameState>>,
    // mut event_writer: (
    //     EventWriter<UnsentServerMessage>,
    // ),
){
    if players.0.len() > 0 {
        let ctx = contexts.ctx_mut();
        let window = windows_q.single();
        let window_width = window.physical_width() as f32;
        let window_height = window.physical_height() as f32;
    
        let lobby_node_width = window_width * 0.8;
        let lobby_node_height = window_height * 0.8;
    
        let x = (window_width - lobby_node_width) / 2.;
        let y = (window_height - lobby_node_height) / 2.;

        egui::Window::new("Server lobby")
        .default_pos(egui::Pos2::new(x, y))
        .default_size(egui::Vec2::new(lobby_node_width, lobby_node_height))
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(lobby_node_height * 0.05);

                for team in players.0.iter(){
                    for player in team.1.iter(){
                        ui.label(
                            egui::RichText::new(format!("{}| {}", team.0, player.1))
                            .size(lobby_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        );

                        ui.add_space(lobby_node_height * 0.05);
                    }
                }

                ui.add_space(lobby_node_height * 0.05);

                if ui.add(
                    egui::Button::new(
                        egui::RichText::new("Start")
                        .size(lobby_node_height * 0.1)
                        .color(egui::Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(0, 0, 0))
                    .stroke(Stroke{
                        width: 0.1,
                        color: Color32::from_rgb(255, 255, 255),
                    })
                    .min_size(egui::Vec2::new(lobby_node_width * 0.8, lobby_node_height * 0.15))
                ).clicked() {
                    let mut channel_id = 60;
                    while channel_id <= 89 {
                        if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::GameInitialized){
                            channel_id += 1;
                        } else {
                            break;
                        }
                    }

                    next_state.set(GameState::MultiplayerAsHost)
                }
            });
        });
    }
}

pub fn show_lobby_as_client(
    windows_q: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
    players: ResMut<PlayerList>,
    mut next_state: ResMut<NextState<GameState>>,
){
    if players.0.len() > 0 {
        let ctx = contexts.ctx_mut();
        let window = windows_q.single();
        let window_width = window.physical_width() as f32;
        let window_height = window.physical_height() as f32;
    
        let lobby_node_width = window_width * 0.8;
        let lobby_node_height = window_height * 0.8;
    
        let x = (window_width - lobby_node_width) / 2.;
        let y = (window_height - lobby_node_height) / 2.;

        egui::Window::new("Server lobby")
        .default_pos(egui::Pos2::new(x, y))
        .default_size(egui::Vec2::new(lobby_node_width, lobby_node_height))
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(lobby_node_height * 0.05);

                for team in players.0.iter(){
                    for player in team.1.iter(){
                        ui.label(
                            egui::RichText::new(format!("{}| {}", team.0, player.1))
                            .size(lobby_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        );

                        ui.add_space(lobby_node_height * 0.05);
                    }
                }
            });
        });
    } else {
        let ctx = contexts.ctx_mut();
        let window = windows_q.single();
        let window_width = window.physical_width() as f32;
        let window_height = window.physical_height() as f32;

        let connection_node_width = window_width * 0.8;
        let connection_node_height = window_height * 0.8;

        let x = (window_width - connection_node_width) / 2.;
        let y = (window_height - connection_node_height) / 2.;
        
        egui::Window::new("Connecting...")
        .default_pos(egui::Pos2::new(x, y))
        .default_size(egui::Vec2::new(connection_node_width, connection_node_height))
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(connection_node_height * 0.5);

                if ui.add(
                    egui::Button::new(
                            egui::RichText::new("Cancel")
                            .size(connection_node_height * 0.1)
                            .color(egui::Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0, 0, 0))
                        .stroke(Stroke{
                            width: 0.1,
                            color: Color32::from_rgb(255, 255, 255),
                        })
                        .min_size(egui::Vec2::new(connection_node_width * 0.8, connection_node_height * 0.15))
                ).clicked() {
                    next_state.set(GameState::MainMenu);
                }

                ui.add_space(connection_node_height * 0.5);
            });
        });
    }
}

#[derive(Component)]
pub struct SuppliesBar {
    pub original_width: f32,
}

#[derive(Component)]
pub struct SuppliesBarHolder {
    pub entity: Entity,
}

#[derive(Component)]
pub struct MaterialsOverallAmountDisplay;

#[derive(Component)]
pub struct HumanResourcesOverallAmountDisplay;

pub fn overall_resources_amount_updating_system (
    material_producers_q: Query<(&MaterialsProductionComponent, &CombatComponent),
    (With<MaterialsProductionComponent>, Without<MaterialsStorageComponent>, Without<HumanResourceStorageComponent>)>,
    material_consumers_q: Query<(&MaterialsStorageComponent, &CombatComponent),
    (Without<MaterialsProductionComponent>, With<MaterialsStorageComponent>, Without<HumanResourceStorageComponent>)>,
    human_resource_producers_q: Query<&SettlementComponent>,
    human_resource_consumers_q: Query<(&HumanResourceStorageComponent, &CombatComponent),
    (Without<MaterialsProductionComponent>, Without<MaterialsStorageComponent>, With<HumanResourceStorageComponent>)>,
    mut materials_displays_q: Query<&mut Text, (With<MaterialsOverallAmountDisplay>, Without<HumanResourcesOverallAmountDisplay>)>,
    mut human_resources_displays_q: Query<&mut Text, (With<HumanResourcesOverallAmountDisplay>, Without<MaterialsOverallAmountDisplay>)>,
    player_data: Res<PlayerData>,
    time: Res<Time>,
    mut elapsed_update_time: Local<u128>,
    network_status: Res<NetworkStatus>,
    mut server: ResMut<QuinnetServer>,
    clients: Res<ClientList>,
){
    if !material_producers_q.is_empty() || !human_resource_producers_q.is_empty() {
        *elapsed_update_time += time.delta().as_millis();

        if *elapsed_update_time >= 1000 {
            *elapsed_update_time = 0;

            let mut materials_amount = (0, 0);
            let mut materials_capacity = (0, 0);
            let mut materials_production_per_second = (0., 0.);

            for material_producer in material_producers_q.iter() {
                if material_producer.1.team == player_data.team {
                    materials_amount.0 += material_producer.0.available_materials;
                    materials_capacity.0 += material_producer.0.materials_storage_capacity;
                    materials_production_per_second.0 += material_producer.0.materials_production_rate as f32 /
                    (material_producer.0.materials_production_speed as f32 / 1000.);
                } else {
                    materials_amount.1 += material_producer.0.available_materials;
                    materials_capacity.1 += material_producer.0.materials_storage_capacity;
                    materials_production_per_second.1 += material_producer.0.materials_production_rate as f32 /
                    (material_producer.0.materials_production_speed as f32 / 1000.);
                }
            }

            let mut materials_consumption_per_second = (0., 0.);

            for material_consumer in material_consumers_q.iter() {
                if material_consumer.1.team == player_data.team {
                    if material_consumer.0.available_resources >= material_consumer.0.materials_storage_capacity {continue;}

                    materials_consumption_per_second.0 +=
                    material_consumer.0.replenishment_amount as f32 / (material_consumer.0.replenishment_cooldown as f32 / 1000.);
                } else {
                    if material_consumer.0.available_resources >= material_consumer.0.materials_storage_capacity {continue;}

                    materials_consumption_per_second.1 +=
                    material_consumer.0.replenishment_amount as f32 / (material_consumer.0.replenishment_cooldown as f32 / 1000.);
                }
            }

            let mut materials_income = (
                (materials_production_per_second.0 - materials_consumption_per_second.0) as i32,
                (materials_production_per_second.1 - materials_consumption_per_second.1) as i32,
            );

            let mut human_resources_amount = (0, 0);
            let mut human_resources_capacity = (0, 0);
            let mut human_resources_production_per_second = (0., 0.);

            for human_resource_producer in human_resource_producers_q.iter() {
                if human_resource_producer.0.team == player_data.team {
                    human_resources_amount.0 += human_resource_producer.0.available_human_resources;
                    human_resources_capacity.0 += human_resource_producer.0.human_resource_storage_capacity;
                    human_resources_production_per_second.0 += human_resource_producer.0.human_resource_production_rate as f32 /
                    (human_resource_producer.0.human_resource_production_speed as f32 / 1000.);
                } else {
                    human_resources_amount.1 += human_resource_producer.0.available_human_resources;
                    human_resources_capacity.1 += human_resource_producer.0.human_resource_storage_capacity;
                    human_resources_production_per_second.1 += human_resource_producer.0.human_resource_production_rate as f32 /
                    (human_resource_producer.0.human_resource_production_speed as f32 / 1000.);
                }
            }

            let mut human_resources_consumption_per_second = (0., 0.);

            for human_resource_consumer in human_resource_consumers_q.iter() {
                if human_resource_consumer.1.team == player_data.team {
                    if human_resource_consumer.0.available_human_resources >= human_resource_consumer.0.human_resource_storage_capacity {continue;}

                    human_resources_consumption_per_second.0 += human_resource_consumer.0.replenishment_amount as f32 /
                    (human_resource_consumer.0.replenishment_cooldown as f32 / 1000.);
                } else {
                    if human_resource_consumer.0.available_human_resources >= human_resource_consumer.0.human_resource_storage_capacity {continue;}

                    human_resources_consumption_per_second.1 += human_resource_consumer.0.replenishment_amount as f32 /
                    (human_resource_consumer.0.replenishment_cooldown as f32 / 1000.);
                }
            }

            let mut human_resources_income = (
                human_resources_production_per_second.0 - human_resources_consumption_per_second.0,
                human_resources_production_per_second.1 - human_resources_consumption_per_second.1,
            );

            let mut delimeter = (" + ", "+");

            if materials_income.0 < 0 {
                delimeter.0 = " - ";
                materials_income.0 *= -1;
            }
            if materials_income.1 < 0 {
                delimeter.1 = " - ";
                materials_income.1 *= -1;
            }

            let team1_materials = materials_amount.0.to_string() + delimeter.0 + &materials_income.0.to_string() + " / " + &materials_capacity.0.to_string();
            let team2_materials = materials_amount.1.to_string() + delimeter.1 + &materials_income.1.to_string() + " / " + &materials_capacity.1.to_string();

            for mut material_display in materials_displays_q.iter_mut() {
                material_display.sections[0].value = team1_materials.clone();
            }

            delimeter = (" + ", "+");

            if human_resources_income.0 < 0. {
                delimeter.0 = " - ";
                human_resources_income.0 *= -1.;
            }
            if human_resources_income.1 < 0. {
                delimeter.1 = " - ";
                human_resources_income.1 *= -1.;
            }

            let team1_human_resources = human_resources_amount.0.to_string() + delimeter.0 + &format!("{:.2}", human_resources_income.0) + " / " + &human_resources_capacity.0.to_string();
            let team2_human_resources = human_resources_amount.1.to_string() + delimeter.1 + &format!("{:.2}", human_resources_income.1) + " / " + &human_resources_capacity.1.to_string();

            for mut human_resources_display in human_resources_displays_q.iter_mut() {
                human_resources_display.sections[0].value = team1_human_resources.clone();
            }

            if matches!(network_status.0, NetworkStatuses::Host) {
                let mut channel_id = 30;
                while channel_id <= 59 {
                    if let Err(_) = server.endpoint_mut().send_group_message_on(clients.0.keys(), channel_id, ServerMessage::ResourceDisplayesUpdated {
                        materials_display: (team1_materials.clone(), team2_materials.clone()),
                        human_resource_display: (team1_human_resources.clone(), team2_human_resources.clone()),
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

pub fn switchable_buildings_ui_manager(
    buildings: Query<(&SwitchableBuilding, &CombatComponent)>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    selection_bounds: Res<SelectionBounds>,
    game_stage: Res<GameStage>,
    mut commands: Commands,
    mut ui_blocker: ResMut<UiBlocker>,
    mut is_menu_opened: Local<bool>,
    network_status: Res<NetworkStatus>,
    mut client: ResMut<QuinnetClient>,
    entity_maps: Res<EntityMaps>,
){
    if mouse_buttons.just_pressed(MouseButton::Left) && matches!(game_stage.0, GameStages::GameStarted) {
        if !ui_button_nodes.is_middle_bottom_node_visible {
            if !selection_bounds.is_ui_hovered {
                if let Some(cursor_ray) = **cursor_ray {
                    let hits = raycast.cast_ray(cursor_ray, &default());

                    let mut is_building_found = false;

                    for hit in hits.iter() {
                        if let Ok(building) = buildings.get(hit.0) {
                            *is_menu_opened = true;
                            ui_blocker.is_bottom_left_node_blocked = true;

                            is_building_found = true;

                            ui_button_nodes.is_left_bottom_node_visible = true;
                            commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);

                            for row in ui_button_nodes.left_bottom_node_rows.iter() {
                                commands.entity(*row).despawn_descendants();
                            }

                            let color;
                            let text;

                            if building.0.0 {
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
                                }).insert(ButtonAction{action: Actions::SwitchBuildingState(hits[0].0)})
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

                            break;
                        }
                    }

                    if !is_building_found && *is_menu_opened && !ui_blocker.is_bottom_left_node_blocked {
                        *is_menu_opened = false;

                        ui_button_nodes.is_left_bottom_node_visible = false;
                        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);

                        for row in ui_button_nodes.left_bottom_node_rows.iter() {
                            commands.entity(*row).despawn_descendants();
                        }
                    }
                }
            }
        }
    }
}

pub fn rebuild_settlement_ui_manager(
    settlements: Query<&SettlementComponent>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    selection_bounds: Res<SelectionBounds>,
    mut ui_blocker: ResMut<UiBlocker>,
    mut commands: Commands,
    game_stage: Res<GameStage>,
    mut is_menu_opened: Local<bool>,
){
    if mouse_buttons.just_pressed(MouseButton::Left) && matches!(game_stage.0, GameStages::GameStarted) {
        if !ui_button_nodes.is_middle_bottom_node_visible {
            if !selection_bounds.is_ui_hovered {
                if let Some(cursor_ray) = **cursor_ray {
                    let hits = raycast.cast_ray(cursor_ray, &default());

                    let mut is_settlement_found = false;

                    for hit in hits.iter() {
                        if let Ok(_settlement) = settlements.get(hit.0) {
                            *is_menu_opened = true;
                            ui_blocker.is_bottom_left_node_blocked = true;

                            is_settlement_found = true;

                            ui_button_nodes.is_left_bottom_node_visible = true;
                            commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);

                            for row in ui_button_nodes.left_bottom_node_rows.iter() {
                                commands.entity(*row).despawn_descendants();
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
                                    background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                                    ..default()
                                }).insert(ButtonAction{action: Actions::RebuildApartments(hits[0].0)})
                                .with_children(|button_parent| {
                                    button_parent.spawn(TextBundle {
                                        text: Text{
                                            sections: vec![TextSection {
                                                value: "RebAp".to_string(),
                                                ..default()
                                            }],
                                            justify: JustifyText::Center,
                                            ..default() 
                                        },
                                        ..default()
                                    });
                                });
                            });

                            break;
                        }
                    }

                    if !is_settlement_found && *is_menu_opened && !ui_blocker.is_bottom_left_node_blocked {
                        *is_menu_opened = false;

                        ui_button_nodes.is_left_bottom_node_visible = false;
                        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);

                        for row in ui_button_nodes.left_bottom_node_rows.iter() {
                            commands.entity(*row).despawn_descendants();
                        }
                    }
                }
            }
        }
    }
}

pub fn ui_nodes_unlocker(//must be last on the updates list
    mut ui_blocker: ResMut<UiBlocker>,
){
    ui_blocker.is_bottom_left_node_blocked = false;
    ui_blocker.is_bottom_middle_node_blocked = false;
}

#[derive(Resource)]
pub struct BuildingHints(pub HashMap<String, String>);

pub fn hint_management_system (
    mut event_reader: EventReader<BuildingButtonHovered>,
    hints: Res<BuildingHints>,
    ui_button_nodes: Res<UiButtonNodes>,
    mut commands: Commands,
    mut current_hint: Local<String>,
){
    for event in event_reader.read() {
        if let Some(hint) = hints.0.get(&event.0) {
            commands.entity(ui_button_nodes.hint_node).insert(Visibility::Visible);

            if *current_hint != event.0 {
                *current_hint = event.0.clone();

                commands.entity(ui_button_nodes.hint_text).insert(Text{
                    sections: vec![TextSection {
                        value: hint.to_string(),
                        ..default()
                    }],
                    justify: JustifyText::Left,
                    ..default()
                });
            }
        }
    }
}

pub fn disembark_button_system(
    transports_q: Query<Entity, (With<InfantryTransport>, With<SelectedUnit>)>,
    mut ui_button_nodes: ResMut<UiButtonNodes>,
    mut ui_blocker: ResMut<UiBlocker>,
    mut is_menu_opened: Local<bool>,
    mut commands: Commands,
){
    if transports_q.is_empty() && *is_menu_opened && !ui_blocker.is_bottom_left_node_blocked {
        *is_menu_opened = false;

        ui_button_nodes.is_left_bottom_node_visible = false;
        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Hidden);

        for row in ui_button_nodes.left_bottom_node_rows.iter() {
            commands.entity(*row).despawn_descendants();
        }
    } else if !ui_blocker.is_bottom_left_node_blocked && !transports_q.is_empty() {
        *is_menu_opened = true;
        ui_blocker.is_bottom_left_node_blocked = true;

        ui_button_nodes.is_left_bottom_node_visible = true;
        commands.entity(ui_button_nodes.left_bottom_node).insert(Visibility::Visible);

        for row in ui_button_nodes.left_bottom_node_rows.iter() {
            commands.entity(*row).despawn_descendants();
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
                background_color: Color::srgba(0.1, 0.1, 0.1, 1.).into(),
                ..default()
            }).insert(ButtonAction{action: Actions::DisembarkInfantry})
            .with_children(|button_parent| {
                button_parent.spawn(TextBundle {
                    text: Text{
                        sections: vec![TextSection {
                            value: "Disbk".to_string(),
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
        *is_menu_opened = false;
    }
}