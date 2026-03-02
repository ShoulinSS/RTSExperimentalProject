use core::f32;

use bevy::{core_pipeline::motion_blur::node, math::{vec2, VectorSpace}, prelude::*, render::render_resource::ShaderType, window::PrimaryWindow};
use bevy_mod_raycast::prelude::*;

use crate::{PlayerData, components::unit::{DisabledUnit, InfantryTransport, IsUnitSelectionAllowed}};

use super::{building::{add_selected_buildings, clear_selected_buildings, CoverComponent, SelectableBuilding, SelectedBuilding, SelectedBuildings}, network::InsertedConnectionData, unit::{add_selected_units, clear_selected_units, CombatComponent, IsArtilleryDesignationActive, IsUnitDeselectionAllowed, SelectableUnit, SelectedUnits, TargetPosition}};

#[derive(Component)]
pub struct CameraComponent{
    pub speed: f32,
}

#[derive(Component)]
pub struct SelectionBox;

#[derive(Resource)]
pub struct TimerResource(pub Timer);

#[derive(Resource)]
pub struct SelectionBounds {
    pub first_point: Vec2,
    pub second_point: Vec2,

    pub first_point_world: Vec3,
    pub second_point_world: Vec3,

    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,

    pub is_selection_active: bool,
    pub is_selection_hidden: bool,
    pub is_ui_hovered: bool,
}

#[derive(Resource)]
pub struct Formation {
    pub points: Vec<Vec3>,
    pub is_formation_active: bool,
}

#[derive(Event)]
pub struct MoveOrderEvent;

pub fn camera_system(
    time: Res<Time>,
    mut timer: ResMut<TimerResource>,
    keys: Res<ButtonInput<KeyCode>>,
    mut camera_q: Query<(&CameraComponent, &mut Transform, &Camera, &GlobalTransform)>,
) {
    timer.0.tick(time.delta());
    for mut camera in camera_q.iter_mut() {
        //Camera movement
        let mut direction = Vec3::ZERO;
        let mut speed = camera.0.speed;

        // forward
        if keys.pressed(KeyCode::KeyW) {
            direction += *camera.1.forward();
        }

        // back
        if keys.pressed(KeyCode::KeyS) {
            direction += *camera.1.back();
        }

        // left
        if keys.pressed(KeyCode::KeyA) {
            direction += *camera.1.left();
        }

        // right
        if keys.pressed(KeyCode::KeyD) {
            direction += *camera.1.right();
        }

        // Accelerate
        if keys.pressed(KeyCode::ShiftLeft) {
            speed *= 4.0;
        }

        let mut movement;

        if keys.pressed(KeyCode::Space) {
            movement = Vec3::new(0., 1., 0.) * speed * time.delta_seconds();
            camera.1.translation += movement;
        }

        if keys.pressed(KeyCode::KeyC) {
            movement = Vec3::new(0., -1., 0.) * speed * time.delta_seconds();
            camera.1.translation += movement;
        }

        direction.y = 0.0;
        movement = direction.normalize_or_zero() * speed * time.delta_seconds();
        camera.1.translation += movement;
        //Camera movement^
    }
}

pub fn handle_mouse_buttons(
    windows_q: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&CameraComponent, &Transform, &Camera, &GlobalTransform)>,
    buttons_keys: (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    selectables: (
        Query<(&Transform, Entity, &CombatComponent), (With<SelectableUnit>, Without<DisabledUnit>)>,
        Query<Entity, With<SelectableBuilding>>,
        Res<PlayerData>,
    ),
    mut target: ResMut<TargetPosition>,
    mut selected_units: ResMut<SelectedUnits>,
    mut selected_buildings: ResMut<SelectedBuildings>,
    mut selection_bounds: ResMut<SelectionBounds>,
    mut formation: ResMut<Formation>,
    cursor_ray: Res<CursorRay>,
    mut raycast: Raycast,
    mut commands: Commands,
    selection_modifiers:(
        Res<IsUnitDeselectionAllowed>,
        Res<IsUnitSelectionAllowed>,
    ),
    mut event_writer: (
        EventWriter<MoveOrderEvent>,
    ),
    unit_containers_q: (
        Query<&CoverComponent>,
        Query<&InfantryTransport>,
    ),
){
    let window = windows_q.single();
    //RMB
    if buttons_keys.0.just_pressed(MouseButton::Right){
        formation.is_formation_active = false;

        formation.points = Vec::new();

        if let Some(cursor_ray) = **cursor_ray {
            let hits = raycast.cast_ray(cursor_ray, &default());

            if hits.len() > 0 {
                target.position = hits[0].1.position();
            }
        }
    }

    if buttons_keys.0.pressed(MouseButton::Right){
        if let Some(cursor_ray) = **cursor_ray {
            let hits = raycast.cast_ray(cursor_ray, &default());

            if hits.len() > 0 {
                formation.points.push(hits[0].1.position());
            }
        }

        if formation.points.len() > 1 {
            if formation.points[0].distance(formation.points[formation.points.len()-1]) > 10. {
                formation.is_formation_active = true;
            }
        }
    }

    if buttons_keys.0.just_released(MouseButton::Right){
        if let Some(cursor_ray) = **cursor_ray {
            let hits = raycast.cast_ray(cursor_ray, &default());

            if hits.len() > 0 && !unit_containers_q.0.get(hits[0].0).is_ok() && !unit_containers_q.1.get(hits[0].0).is_ok() {
                event_writer.0.send(MoveOrderEvent);
            }
        }
    }
    //RMB^

    //LMB
    if buttons_keys.0.just_pressed(MouseButton::Left){
        selection_bounds.first_point = Vec2::ZERO;
        selection_bounds.second_point = Vec2::ZERO;
        selection_bounds.first_point_world = Vec3::ZERO;
        selection_bounds.second_point_world = Vec3::ZERO;
        
        if let Some(cursor_pos) = window.cursor_position(){
            selection_bounds.first_point = cursor_pos;
        }

        // if let Some(cursor_ray) = **cursor_ray {
        //     let hits = raycast.cast_ray(cursor_ray, &default());

        //     if hits.len() > 0 {
        //         selection_bounds.first_point_world = hits[0].1.position();
        //     }
        // }

        if !buttons_keys.1.pressed(KeyCode::ControlLeft) && !selection_bounds.is_ui_hovered && selection_modifiers.0.0 {
            clear_selected_units(&mut selected_units, &mut commands, &selectables.0);

            clear_selected_buildings(&mut selected_buildings, &mut commands, &selectables.1);
        }
    }

    if buttons_keys.0.pressed(MouseButton::Left){
        if let Some(cursor_pos) = window.cursor_position(){
            selection_bounds.second_point = cursor_pos;
            if selection_bounds.first_point.distance(selection_bounds.second_point) > 10. {
                if !selection_bounds.is_selection_active {
                    selection_bounds.is_selection_active = true;
                    selection_bounds.is_selection_hidden = false;
                }
            }
        }
    }

    if buttons_keys.0.just_released(MouseButton::Left){
        if selection_bounds.is_selection_active {
            // if let Some(cursor_ray) = **cursor_ray {
            //     let hits = raycast.cast_ray(cursor_ray, &default());
    
            //     if hits.len() > 0 {
            //         selection_bounds.second_point_world = hits[0].1.position();
            //     }
            // }

            let mut units_to_select: Vec<Entity> = Vec::new();

            let min_x = selection_bounds.first_point.x.min(selection_bounds.second_point.x);
            let max_x = selection_bounds.first_point.x.max(selection_bounds.second_point.x);
            let min_y = selection_bounds.first_point.y.min(selection_bounds.second_point.y);
            let max_y = selection_bounds.first_point.y.max(selection_bounds.second_point.y);
        
            for unit in selectables.0.iter() {
                if unit.2.team != selectables.2.team {continue;}
                
                let position = unit.0.translation;

                let camera = camera_q.single();
                if let Some(screen_pos) = camera.2.world_to_viewport(camera.3, position) {
                    if
                    screen_pos.x >= min_x && screen_pos.x <= max_x &&
                    screen_pos.y >= min_y && screen_pos.y <= max_y {
                            units_to_select.push(unit.1);
                    }
                }

                // if
                // position.x >= min_x && position.x <= max_x &&
                // position.z >= min_z && position.z <= max_z {
                //     units_to_select.push(unit.1);
                // }
            }
            
            if !units_to_select.is_empty() {
                clear_selected_units(&mut selected_units,&mut commands, &selectables.0);

                clear_selected_buildings(&mut selected_buildings, &mut commands, &selectables.1);

                if selection_modifiers.1.0 {
                    add_selected_units(units_to_select, &mut selected_units, &mut commands, &selectables.0);
                }
            }
            else{
                clear_selected_units(&mut selected_units,&mut commands, &selectables.0);

                clear_selected_buildings(&mut selected_buildings, &mut commands, &selectables.1);
            }
        }
        else{
            if let Some(cursor_ray) = **cursor_ray {
                let hits = raycast.cast_ray(cursor_ray, &default());
    
                if hits.len() > 0 && !selection_bounds.is_ui_hovered {
                    if selectables.0.get(hits[0].0).is_ok() && selected_buildings.buildings.is_empty() && selection_modifiers.1.0 {
                        add_selected_units(vec![hits[0].0], &mut selected_units, &mut commands, &selectables.0);
                    } else if selectables.1.get(hits[0].0).is_ok() && selected_units.platoons.is_empty() && selection_modifiers.1.0 {
                        add_selected_buildings(vec![hits[0].0], &mut selected_buildings, &mut commands, &selectables.1);
                    }
                }
            }
        }

        selection_bounds.is_selection_active = false;
    }
    //LMB^
}

pub fn update_selection_box(
    mut nodes_q: Query<&mut Style, With<SelectionBox>>,
    mut selection_bounds: ResMut<SelectionBounds>,
){
    if selection_bounds.is_selection_active {
        let mut selection_box = nodes_q.single_mut();

        if selection_bounds.first_point.x < selection_bounds.second_point.x {
            selection_bounds.min_x = selection_bounds.first_point.x;
            selection_bounds.max_x = selection_bounds.second_point.x;
        }
        else{
            selection_bounds.min_x = selection_bounds.second_point.x;
            selection_bounds.max_x = selection_bounds.first_point.x;
        }

        if selection_bounds.first_point.y < selection_bounds.second_point.y {
            selection_bounds.min_y = selection_bounds.first_point.y;
            selection_bounds.max_y = selection_bounds.second_point.y;
        }
        else{
            selection_bounds.min_y = selection_bounds.second_point.y;
            selection_bounds.max_y = selection_bounds.first_point.y;
        }

        selection_box.left = Val::Px(selection_bounds.min_x);
        selection_box.top = Val::Px(selection_bounds.min_y);

        selection_box.width = Val::Px(selection_bounds.max_x - selection_bounds.min_x);
        selection_box.height = Val::Px(selection_bounds.max_y - selection_bounds.min_y);
    }
    else if !selection_bounds.is_selection_hidden {
        selection_bounds.is_selection_hidden = true;
        let mut selection_box = nodes_q.single_mut();

        selection_box.left = Val::Px(0.);
        selection_box.top = Val::Px(0.);

        selection_box.width = Val::Px(0.);
        selection_box.height = Val::Px(0.);
    }
}