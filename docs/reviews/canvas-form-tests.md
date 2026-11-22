# Tests for removing the sketch canvases

Inventory at the merge of the viewport drag work, `9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9`. Each row links to the original assertion. The disposition applies to that witness, even if the enclosing module disappears. “Keep fixture” work changes setup only. A proposed port that encounters a missing interaction stays pending the decision in the pull request.

## Reproducing window and CLI checks

For a session or file comparison, read `content_version` with `ferritecad inspect SOURCE --json`, issue `ferritecad edit SOURCE --expect-version VERSION --request CHANGE.json -o NEXT`, then use the original independent SQL, cold rebuild, named topology and STL/FBX/STEP assertions for that test. Use the same UUIDs on both sides of an edit. Retain CLI reference templates when the test compares independently created models. Obtain GUI outputs through the real window. For ownership-only events (Cancel, tab switch, pointer, rejected or late reply) the CLI has no event to replay. Its comparison is the unchanged model and files. Do not substitute a synthetic request for a missing user interaction.

| Port or retention method | Exact scope and CLI reference |
| --- | --- |
| Retire | Remove the named legacy widget, catalogue or local gesture assertion with its consumer. |
| Window | Use an open Edit sketch… draft for edit exclusion, Cancel and session acceptance. Use the retained height or Revolve angle form when the invariant requires literal text or Save a copy. Pure interaction has no CLI submission. |
| Drawing | Open Edit sketch… on the same accepted scene. Assert the precise remaining action list, Delete, curve IDs, strokes and row text. The no-profile case still refuses both sweeps. No CLI operation is issued. |
| View | Use Select in Edit sketch… and the real input owner. Keep the camera exclusion or session publication assertions. For published geometry replay the same set_curve_geometry transaction and exports. |
| Geometry | Edit sketch… → Select → edit the same saved curve IDs → Finish. Replay ordered set_curve_geometry changes via edit. Keep source cells, stored direction, identity, geometry, history and all artifact comparisons. See the numeric-input and copy gaps. |
| Rule | Edit sketch… → select point, line, circle or saved rule → rule, Set or Delete → Finish. Replay add_constraint, remove_constraint and set_expression, plus the geometry writes required by Finish. Preserve the exact asserted identities, transaction and refusal. See the rule-identity and solver gaps. |
| New | Use New… Empty or Sample for the New owner. For model creation, continue on XY with New sketch… → Finish → Extrude… or Revolve… → Apply. Replay create and versioned edit on each accepted model. Keep the original picture-refusal, undo, cleanup and late-answer checks where applicable. |
| Ownership | Park Edit sketch… with changed selection, geometry or a rule on the same tab. Use surviving New… Empty/Sample for New, and a retained numeric form for saved literal fields. Drive the same Close/Quit/Open/Recover or switch and assert all owners, question addresses and history. Only accepted model steps have a CLI replay. |
| Angle | Use Edit sketch… for the profile and keep Edit Revolve angle… for the sweep. Replay generic edit on the sketch followed by set_expression on the Revolve angle. Inspect, cold rebuild and export both outputs. |
| Tree | Use the same workspace/tree with Empty, assert New sketch… and the exact remaining commands and layout. No CLI side for a button list. |
| Fixture | Keep the original checks. Replace only retired fixture/catalogue access with stored object UUIDs, accepted facts or a moved test builder. Any pre-existing CLI call and comparator stay. |
| Projection | Keep coverage of the shared picker, numeric conversion or label contract where a viewport reader remains. Port the surviving assertion to its viewport reader. Drop only the old canvas wrapper. |
| Keep | Keep the existing path, fixture behaviour and expectations. No additional CLI invocation. |

## Counts

All three configurations register the same 991 window witnesses in the merge source. The preceding timed run registered 989. The last two drag tests are retained. A successful native guard return in stub has no native coverage. Counts below describe the proposed disposition of existing tests. New absence checks will be counted separately after approval.

| Window configuration | Base registered | Delete | Port | Keep | Existing registrations after proposed removals |
| --- | ---: | ---: | ---: | ---: | ---: |
| stub | 991 | 192 | 214 | 585 | 799 |
| no-solver | 991 | 192 | 214 | 585 | 799 |
| solver | 991 | 192 | 214 | 585 | 799 |

All 117 tests in the UI library remain. CLI, jobs and other library suites remain. The detailed rows below include the dependent session and feature scenarios alongside the retired modules. Other app tests use none of the retiring inputs and belong to Keep.

## Recorded time before migration

The unchanged proposed port witnesses have per-test timings in the previous 14e serial window logs at `c75b9c9b`. Times are elapsed test-body seconds within each configuration. They include guard returns, exclude compilation and do not measure a port. Timing the same witnesses after each approved migration is required before accepting it.

| Port set | Stub seconds | OCCT seconds | OCCT + solver seconds |
| --- | ---: | ---: | ---: |
| All 214 proposed ports | 103.19 | 279.56 | 338.69 |
| `sessions::tests::tabs::drafts::open_new_recover::native_open_new_recover_beside_forms_then_apply_save_and_compare_with_the_command_line` | 0.00 | 15.58 | 15.90 |
| `sessions::tests::tabs::drafts::quit_new::native_returned_new_creates_then_publishes_and_matches_cli_sql_uuid_cold_geometry_exports` | 0.00 | 11.18 | 11.05 |
| `sessions::tests::tabs::drafts::native_tab_drafts_scenario_on_session_files_passes_the_comparator_and_its_controls` | 0.00 | 10.39 | 10.83 |

## Tests by source module

The symbol identifies the exact invariant under review. The method column supplies its replacement stimulus or the reason for retirement. Where a legacy interaction and a shared guarantee occur in one test, preserve the shared part in the named port. A Keep row in a native artifact comparator also requires retaining its real producer.


### ferritecad-app/src/accepted_scene_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_accepted_scene_hands_a_form_the_real_presentation_of_its_own_version`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/accepted_scene_tests.rs#L93) | Port | Use the accepted presentation (including version/document rejection and follow). Preserve the tested absence or retention of the drawing and facts across failed publication. No CLI side. |
| [`a_form_is_never_handed_the_drawing_of_another_version_or_document`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/accepted_scene_tests.rs#L149) | Port | Use the accepted presentation (including version/document rejection and follow). Preserve the tested absence or retention of the drawing and facts across failed publication. No CLI side. |
| [`a_kept_picture_follows_its_version_and_a_failed_arrival_keeps_the_drawing_and_the_facts`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/accepted_scene_tests.rs#L201) | Port | Use the accepted presentation (including version/document rejection and follow). Preserve the tested absence or retention of the drawing and facts across failed publication. No CLI side. |

### ferritecad-app/src/chamfers/tests/add_session.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`add_chamfer_reads_current_fields_request_history_stays_separate_and_copy_is_clean_only`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/add_session.rs#L98) | Keep | Keep |
| [`add_chamfer_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/add_session.rs#L207) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`add_chamfer_dirty_discovery_refusals_keep_draft_status_dismissal_and_closed_classes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/add_session.rs#L297) | Keep | Keep |

### ferritecad-app/src/chamfers/tests/session_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`distance_current_text_draft_history_noop_clean_copy_and_add_exclusion`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/session_apply.rs#L121) | Keep | Fixture |
| [`distance_busy_guard_proves_changed_idle_request_and_excludes_other_work`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/session_apply.rs#L224) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`distance_dirty_discovery_copy_reason_refusal_retains_text_and_acceptance_closes_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers/tests/session_apply.rs#L328) | Keep | Keep |

### ferritecad-app/src/chamfers.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_request_history_steps_back_and_forward_and_forgets_a_discarded_future`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/chamfers.rs#L671) | Keep | Keep |

### ferritecad-app/src/commands/tests/curve_edits.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`empty_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/curve_edits.rs#L123) | Port | Use the real Edit sketch… form and its accepted curve identities. Keep the three fixture-specific tree, stroke and Delete assertions. Remove the old Segment-row wording only. |
| [`open_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/curve_edits.rs#L127) | Port | Use the real Edit sketch… form and its accepted curve identities. Keep the three fixture-specific tree, stroke and Delete assertions. Remove the old Segment-row wording only. |
| [`scrambled_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/curve_edits.rs#L131) | Port | Use the real Edit sketch… form and its accepted curve identities. Keep the three fixture-specific tree, stroke and Delete assertions. Remove the old Segment-row wording only. |
| [`repair_document_attributes_missing_profile_to_the_consumer_and_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/curve_edits.rs#L135) | Keep | Keep |

### ferritecad-app/src/commands/tests/native.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_gpu_faces_vertices_and_fillet_edges_select_their_producer_and_expand_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/native.rs#L27) | Keep | Keep |
| [`native_gpu_imported_entity_selects_the_stored_import_object`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/native.rs#L174) | Keep | Keep |

### ferritecad-app/src/commands/tests/rollback.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`rollback_addition_facts_use_shown_and_later_existing_edit_routes_remain_available`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/rollback.rs#L214) | Keep | Keep |
| [`native_rollback_widgets_restore_exact_versions_scenes_and_pick_the_shown_producer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/rollback.rs#L255) | Keep | Keep |
| [`native_rollback_fillet_form_inserts_at_the_shown_result_and_undo_restores_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/rollback.rs#L360) | Keep | Keep |

### ferritecad-app/src/commands/tests/widgets.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`real_tree_buttons_context_menus_and_shortcuts_share_the_command_catalogue`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/widgets.rs#L94) | Keep | Keep |
| [`the_entity_menu_uses_the_same_parameter_form_exception_and_operation_guard`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/widgets.rs#L250) | Keep | Keep |
| [`the_entity_menu_closes_on_its_button_a_click_elsewhere_and_escape_only`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/widgets.rs#L337) | Keep | Keep |
| [`delete_shortcut_respects_text_focus_and_a_disabled_model_panel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests/widgets.rs#L414) | Keep | Keep |

### ferritecad-app/src/commands/tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_producer_requires_one_complete_meaning_not_one_distinct_producer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L10) | Keep | Keep |
| [`every_command_shares_idle_refusals_and_states_its_object_requirement`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L35) | Keep | Keep |
| [`a_pending_confirmation_uses_the_same_idle_gate_as_an_open_editor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L154) | Keep | Keep |
| [`confirmation_cancellation_sends_nothing_and_stale_confirmation_refuses`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L171) | Keep | Keep |
| [`catalogue_carries_the_same_labels_shortcuts_and_reasons_for_each_surface`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L213) | Keep | Keep |
| [`rollback_catalogue_uses_the_body_marker_and_preserves_every_idle_guard`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/commands/tests.rs#L252) | Keep | Keep |

### ferritecad-app/src/constraints/tests/angle.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_directed_angle_is_two_explicit_vectors_and_a_signed_angle_in_one_step_through_the_real_widgets`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/angle.rs#L80) | Delete | Retire |
| [`a_stored_angle_is_replaced_by_another_in_one_exact_remove_and_add`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/angle.rs#L340) | Delete | Retire |
| [`native_directed_angle_form_worker_and_cli_publish_the_same_geometry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/angle.rs#L457) | Port | Port once-only publication and independent CLI comparison. Reversed vector endpoints cannot be selected independently when adding Angle. Preserve the exact rule using an existing CLI-built saved-rule fixture where it tests subsequent editing. Lead decision needed if creation of that exact rule is itself asserted. |

### ferritecad-app/src/constraints/tests/canvas.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_click_fills_the_fields_the_lists_fill_for_each_explicit_target_and_only_them`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L252) | Delete | Retire |
| [`the_lists_and_the_canvas_show_one_selection`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L344) | Delete | Retire |
| [`typed_inputs_history_and_requests_wait_for_an_explicit_action`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L420) | Port | Window |
| [`a_reversed_line_is_picked_by_its_start_and_end_not_by_the_screen_side`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L474) | Delete | Retire |
| [`zoom_pan_and_fit_move_only_the_view_and_a_pick_follows_the_view_at_one_and_two_pixels_per_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L607) | Delete | Retire |
| [`busy_queued_covered_outside_and_unfocused_canvases_change_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L712) | Delete | Retire |
| [`each_missing_drawing_is_stated_in_the_form_and_the_lists_still_pick`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L844) | Delete | Retire |
| [`rules_still_pending_are_not_in_the_drawing_until_they_are_applied`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L902) | Delete | Retire |
| [`a_pan_holds_the_window_and_ends_with_the_canvas_that_stops_being_drawn`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L933) | Port | Window |
| [`the_canvas_never_reaches_the_3d_camera_and_a_viewport_drag_still_crosses_the_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L1003) | Port | View |
| [`the_form_keeps_its_canvas_its_modes_and_its_actions_reachable_at_768_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L1197) | Delete | Retire |
| [`a_press_is_a_click_only_while_it_stays_put_and_nothing_takes_it_away`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L2002) | Delete | Retire |
| [`a_circle_is_one_curve_picked_by_its_ring_or_its_centre_and_a_single_circle_has_no_pair`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L2088) | Delete | Retire |
| [`a_pick_on_the_canvas_brings_its_row_of_a_long_list_into_view`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/canvas.rs#L2168) | Delete | Retire |

### ferritecad-app/src/constraints/tests/collinear.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_collinear_is_one_step_on_a_picked_pair_and_reads_neither_vector_direction`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/collinear.rs#L60) | Delete | Retire |
| [`a_stored_collinear_is_replaced_by_a_parallel_in_one_exact_remove_and_add`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/collinear.rs#L191) | Delete | Retire |
| [`native_collinear_form_worker_and_cli_publish_the_same_geometry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/collinear.rs#L309) | Port | Rule |

### ferritecad-app/src/constraints/tests/diameter.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_circle_diameter_is_the_half_radius_in_one_step_through_the_real_widgets`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/diameter.rs#L174) | Delete | Retire |
| [`running_jobs_freeze_focused_constraint_numbers_until_editing_resumes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/diameter.rs#L339) | Port | View |
| [`a_stored_radius_is_replaced_by_a_diameter_in_one_exact_remove_and_add`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/diameter.rs#L384) | Delete | Retire |
| [`ring_diameters_follow_the_picked_uuid_not_the_stored_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/diameter.rs#L431) | Delete | Retire |
| [`native_diameter_form_worker_and_cli_publish_the_same_radii_ids_sql_and_solid`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/diameter.rs#L524) | Port | Rule |

### ferritecad-app/src/constraints/tests/dimension.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_label_states_the_saved_target_and_selects_exactly_its_rule_only_under_the_dimension_target`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L257) | Delete | Retire |
| [`the_labels_the_lines_list_and_the_persisted_rows_show_one_selection`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L387) | Delete | Retire |
| [`replace_dimension_prepares_one_step_updates_it_in_place_and_the_saved_number_takes_it_back`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L452) | Port | Rule |
| [`replace_dimension_is_the_replace_length_of_the_list_and_never_a_tolerance`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L576) | Delete | Retire |
| [`a_dimension_chosen_on_the_canvas_brings_its_row_and_editor_into_view_in_a_long_list`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L641) | Delete | Retire |
| [`a_cancelled_save_or_a_stale_reply_keeps_the_text_the_selection_and_the_prepared_replacement`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L668) | Port | Use a changed draft on the same model and exercise both unmatched and matching async destinations. Keep refusal, original model and version exact. |
| [`a_refused_replacement_keeps_the_typed_text_the_selection_and_the_history_exactly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L730) | Port | Keep the session refusal/atomicity and preserve its exact text. A form that refuses before submission cannot witness a worker refusal. Use the existing owner transaction test for that layer as well as the new form for the real pre-submit refusal. |
| [`a_circle_radius_is_replaced_as_the_radius_whether_the_number_is_a_radius_or_a_diameter`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L770) | Delete | Retire |
| [`labels_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L905) | Delete | Retire |
| [`a_label_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L1027) | Port | View |
| [`the_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L1207) | Delete | Retire |
| [`without_a_drawing_the_saved_rows_still_select_and_replace_and_other_profiles_have_no_dimension`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L1374) | Delete | Retire |
| [`two_forms_over_one_document_keep_their_own_selection_text_and_pending_replacement`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L1461) | Port | Rule |
| [`overlapping_labels_are_reached_in_stored_order_and_the_list_reaches_the_one_underneath`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/dimension.rs#L1497) | Delete | Retire |

### ferritecad-app/src/constraints/tests/marker.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_marker_states_its_kind_and_selects_exactly_its_rule_only_under_the_rule_target`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L440) | Delete | Retire |
| [`the_selected_rule_marks_exactly_the_references_it_stores_and_a_reversed_segment_stays_as_stored`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L573) | Delete | Retire |
| [`the_markers_the_lines_list_the_persisted_rows_and_the_other_labels_show_one_selection_and_the_pair_slots_never_move`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L692) | Delete | Retire |
| [`remove_constraint_prepares_one_step_once_and_undo_redo_and_clear_return_exactly_the_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L801) | Port | Rule |
| [`a_cancelled_save_or_a_stale_reply_keeps_the_selected_rule_and_the_prepared_removal`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L955) | Port | Use a changed draft on the same model and exercise both unmatched and matching async destinations. Keep refusal, original model and version exact. |
| [`markers_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1032) | Delete | Retire |
| [`a_marker_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1140) | Port | View |
| [`overlapping_markers_are_reached_in_saved_order_the_target_decides_what_is_on_top_and_the_list_reaches_the_one_underneath`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1372) | Delete | Retire |
| [`the_markers_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1539) | Delete | Retire |
| [`without_a_drawing_the_rule_rows_still_select_and_remove_and_closure_and_other_profiles_have_no_marker`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1674) | Delete | Retire |
| [`two_forms_over_one_document_keep_their_own_rule_selection_and_pending_removals`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1776) | Port | Rule |
| [`the_target_row_keeps_five_targets_and_fit_beside_rule_in_one_row_and_dimension_keeps_its_meaning`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1808) | Delete | Retire |
| [`a_single_circles_fixed_centre_is_a_marker_beside_its_radius_label_and_marks_the_centre`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/marker.rs#L1996) | Delete | Retire |

### ferritecad-app/src/constraints/tests/revolve.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`revolve_constraint_widgets_name_the_revolve_build_and_replace_a_length`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/revolve.rs#L210) | Delete | Retire |
| [`native_revolve_constraint_worker_and_cli_publish_the_same_solved_sector`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/revolve.rs#L357) | Port | Rule |
| [`axis_closed_constraint_widgets_name_the_axis_line_and_keep_the_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/revolve.rs#L544) | Delete | Retire |
| [`native_axis_closed_worker_and_cli_publish_the_same_solved_shaft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/revolve.rs#L697) | Port | Rule |

### ferritecad-app/src/constraints/tests/session_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_open_form_applies_through_the_windows_own_predicate_and_keeps_its_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L75) | Port | Rule |
| [`nothing_else_running_or_open_lets_the_command_start`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L126) | Port | Window |
| [`an_empty_draft_offers_no_apply_and_keeps_the_redo_of_the_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L170) | Port | Rule |
| [`with_unsaved_changes_save_copy_is_withheld_with_words_and_apply_is_not`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L200) | Port | The old form refuses Save a copy on a dirty source. Use the surviving Revolve angle or height copy workflow for that guard and its exact text. Finishing a sketch followed by Save As changes the open model. |
| [`a_new_accepted_version_ends_the_draft_and_keeps_what_the_window_said`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L229) | Port | Window |
| [`the_form_opens_on_a_document_with_unsaved_changes_only_when_the_window_says_so`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/session_apply.rs#L289) | Port | Window |

### ferritecad-app/src/constraints/tests/turn_label.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_label_states_the_saved_signed_target_and_selects_exactly_its_rule_with_both_vectors_only_under_the_dimension_target`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L250) | Delete | Retire |
| [`the_labels_the_lines_list_and_the_persisted_rows_show_one_selection_and_the_pair_slots_never_move`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L540) | Delete | Retire |
| [`replace_dimension_for_a_saved_angle_prepares_one_step_keeps_both_vectors_updates_in_place_and_the_saved_angle_takes_it_back`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L613) | Delete | Retire |
| [`replace_dimension_is_the_remove_and_add_of_the_form_and_never_a_tolerance`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L766) | Delete | Retire |
| [`a_refused_angle_keeps_the_typed_text_the_selection_and_the_history_exactly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L840) | Port | Keep the session refusal/atomicity and preserve its exact text. A form that refuses before submission cannot witness a worker refusal. Use the existing owner transaction test for that layer as well as the new form for the real pre-submit refusal. |
| [`a_cancelled_save_or_a_stale_reply_keeps_the_text_the_selection_and_the_prepared_angle`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L890) | Port | Use a changed draft on the same model and exercise both unmatched and matching async destinations. Keep refusal, original model and version exact. |
| [`labels_follow_zoom_pan_and_fit_and_are_hit_exactly_where_they_are_drawn`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L972) | Delete | Retire |
| [`an_angle_label_press_dies_with_its_surface_and_busy_queued_covered_or_unfocused_forms_take_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1087) | Port | View |
| [`overlapping_labels_are_reached_in_saved_order_and_the_list_reaches_the_one_underneath`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1296) | Delete | Retire |
| [`the_angle_labels_the_editor_and_the_actions_stay_reachable_at_768_points_in_the_worst_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1406) | Delete | Retire |
| [`without_a_drawing_the_angle_rows_still_select_and_replace_and_other_profiles_have_no_angle`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1534) | Delete | Retire |
| [`two_forms_over_one_document_keep_their_own_angle_selection_text_and_pending_replacement`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1588) | Port | Rule |
| [`an_angle_typed_as_it_is_displayed_is_the_saved_angle_exactly_and_the_one_that_is_not_says_so`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints/tests/turn_label.rs#L1678) | Port | Keep editable_degrees round-trip witnesses. Move them to the viewport dimension field. Discard the retired label hitbox or old exact UI sentence only. |

### ferritecad-app/src/constraints.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`line_length_widgets_validate_input_without_changing_history_until_apply`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L2642) | Delete | Retire |
| [`replace_line_length_widgets_build_one_remove_add_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L2727) | Delete | Retire |
| [`constraint_history_widgets_restore_requests_and_branch_only_on_changes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L2973) | Delete | Retire |
| [`constraint_history_remove_widgets_restore_exact_persisted_ids`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3067) | Delete | Retire |
| [`constraint_history_bounds_both_stacks_and_retains_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3138) | Delete | Retire |
| [`constraint_editor_keeps_actions_reachable_with_many_pending_additions`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3179) | Delete | Retire |
| [`constraint_widgets_keep_draft_on_save_cancel_refusal_and_stale_reply`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3344) | Port | Use a changed draft on the same model and exercise both unmatched and matching async destinations. Keep refusal, original model and version exact. |
| [`annulus_widgets_name_both_roles_and_build_one_parametric_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3513) | Delete | Retire |
| [`circle_widgets_add_a_radius_and_a_fixed_centre_as_one_request_each`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3713) | Delete | Retire |
| [`native_circle_constraint_worker_and_cli_publish_the_same_solid`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L3904) | Port | Rule |
| [`native_annular_constraint_worker_and_cli_publish_the_same_ring`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L4087) | Port | Rule |
| [`fixed_point_widgets_pick_an_endpoint_and_replace_the_stored_pin_in_one_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L4266) | Delete | Retire |
| [`equal_length_widgets_pick_two_lines_and_name_the_persisted_pair`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L4465) | Delete | Retire |
| [`line_relation_widgets_pick_two_lines_and_name_the_persisted_relation`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L4618) | Delete | Retire |
| [`native_fixed_point_worker_and_cli_pin_the_same_solved_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L4759) | Port | Rule |
| [`native_equal_length_worker_and_cli_tie_the_same_solved_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L5078) | Port | Rule |
| [`native_line_relations_worker_and_cli_orient_the_same_solved_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L5378) | Port | Rule |
| [`native_constraint_worker_and_cli_preserve_model_and_solved_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L5697) | Port | Rule |
| [`native_line_length_worker_and_cli_preserve_model_and_solved_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L5701) | Port | Rule |
| [`native_fillet_base_constraint_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L6255) | Port | Rule |
| [`native_chamfer_base_constraint_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L6383) | Port | Rule |
| [`native_two_fillet_base_constraint_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/constraints.rs#L6503) | Port | Rule |

### ferritecad-app/src/creates.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`nothing_is_asked_before_new_is_pressed`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1033) | Keep | Keep |
| [`the_form_offers_the_same_plate_the_command_line_does`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1046) | Keep | Keep |
| [`a_form_taken_back_makes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1065) | Keep | Keep |
| [`a_size_that_is_not_a_number_is_reported_in_the_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1081) | Keep | Keep |
| [`the_form_stays_until_the_new_document_is_accepted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1105) | Keep | Keep |
| [`the_window_route_makes_an_untitled_session_and_writes_no_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1153) | Keep | Keep |
| [`an_empty_document_is_a_document`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1192) | Keep | Keep |
| [`a_taken_name_is_refused_by_the_first_save_without_a_replacement`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1216) | Keep | Keep |
| [`a_size_the_document_refuses_leaves_nothing_behind`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1252) | Keep | Keep |
| [`a_stale_answer_is_not_acted_on`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1281) | Keep | Keep |
| [`a_creation_in_flight_is_the_only_thing_that_withdraws_new`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1328) | Keep | Keep |
| [`stub_creation_is_refused_at_the_picture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1371) | Keep | Keep |
| [`both_clients_write_the_same_plate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1663) | Keep | Keep |
| [`both_clients_write_the_same_empty_document`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1723) | Keep | Keep |
| [`cancel_waits_for_the_worker_and_accepts_nothing_it_made`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1752) | Keep | Keep |
| [`shutdown_cancels_and_joins_creation_with_its_scratch_removed`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1805) | Keep | Keep |
| [`a_question_is_bound_to_the_new_it_was_asked_over`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1863) | Keep | Keep |
| [`stopping_new_ends_its_draft_and_leaves_the_worker_to_end_on_its_own`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L1937) | Port | Keep queued/running work, resubmission and late-result checks on Empty/Sample. Remove only the retired drawing fragment within the same test. |
| [`an_abandoned_creation_never_answers_for_a_newer_one`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L2005) | Keep | Keep |
| [`both_clients_reopen_rebuild_resolve_and_export_the_saved_plate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/creates.rs#L2060) | Keep | Keep |

### ferritecad-app/src/cuts/tests/add_session.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`add_cut_reads_current_fields_draft_history_stays_separate_and_copy_is_clean_only`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/add_session.rs#L109) | Keep | Keep |
| [`add_cut_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/add_session.rs#L218) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`add_cut_dirty_discovery_refusals_keep_text_status_dismissal_and_unsupported_bodies`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/add_session.rs#L345) | Keep | Keep |

### ferritecad-app/src/cuts/tests/session_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`cut_apply_current_fields_draft_history_noop_and_add_exclusion`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/session_apply.rs#L166) | Keep | Fixture |
| [`cut_apply_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/session_apply.rs#L223) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`cut_apply_dirty_discovery_copy_refusals_status_and_scene_dismissal`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/cuts/tests/session_apply.rs#L311) | Keep | Fixture |

### ferritecad-app/src/dialogs.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`null_panel_constructor_is_a_failure_instead_of_unwinding_the_viewer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/dialogs.rs#L256) | Keep | Keep |
| [`unrelated_panics_are_not_disguised_as_dialog_failures`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/dialogs.rs#L278) | Keep | Keep |
| [`failure_is_visible_cancel_is_silent_and_only_selection_reaches_work`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/dialogs.rs#L303) | Keep | Keep |
| [`only_a_clear_save_or_discard_is_not_a_cancel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/dialogs.rs#L352) | Keep | Keep |

### ferritecad-app/src/edits.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`one_source_ui_and_cli_preserve_all_ids_rebuild_and_export_the_changed_model`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L561) | Keep | Keep |
| [`imported_blobs_unknown_tables_and_unselected_payloads_survive_edit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L712) | Keep | Keep |
| [`hidden_row_identity_and_stored_triggers_refuse_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L772) | Keep | Keep |
| [`foreign_key_actions_refuse_publication_without_changing_extension_data`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L880) | Keep | Keep |
| [`cancellation_before_and_after_publish_and_stale_answers_keep_honest_outcomes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L945) | Keep | Keep |
| [`published_output_and_open_failure_are_separate_from_the_accepted_scene`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L1007) | Keep | Keep |
| [`cancelling_the_form_and_a_closed_save_dialog_do_no_work`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L1069) | Keep | Keep |
| [`shutdown_cancels_and_joins_the_edit_worker_before_it_can_publish`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L1096) | Keep | Keep |
| [`the_height_form_offers_apply_and_withholds_the_copy_workflow_while_dirty`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/edits.rs#L1289) | Keep | Keep |

### ferritecad-app/src/fillets/tests/add_session.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`add_fillet_reads_current_selection_confirm_is_draft_only_and_copy_is_clean_only`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/add_session.rs#L83) | Keep | Keep |
| [`add_fillet_busy_guard_proves_idle_request_then_excludes_other_work_and_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/add_session.rs#L176) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`add_fillet_dirty_discovery_refusals_keep_draft_status_dismissal_and_full_plate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/add_session.rs#L272) | Keep | Keep |

### ferritecad-app/src/fillets/tests/session_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`radius_current_text_confirmation_noop_clean_copy_and_add_exclusion`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/session_apply.rs#L127) | Keep | Fixture |
| [`radius_busy_guard_proves_changed_idle_request_and_excludes_other_work`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/session_apply.rs#L181) | Port | Use the existing no-solver or mock fixture for the competing open editor, e.g. the height form. Keep every exclusion, changed idle request and dirty guard. |
| [`radius_dirty_discovery_refusal_retains_text_and_acceptance_closes_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets/tests/session_apply.rs#L259) | Keep | Keep |

### ferritecad-app/src/fillets.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_fillet_radius_widgets_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets.rs#L930) | Keep | Keep |
| [`native_sequential_radius_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/fillets.rs#L1046) | Keep | Keep |

### ferritecad-app/src/main.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`new_waits_for_load_export_and_export_confirmation`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L18704) | Keep | Keep |
| [`the_create_predicate_lets_open_forms_create_and_excludes_other_work`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L18762) | Keep | Keep |
| [`new_command_preserves_accepted_scene_until_the_candidate_is_shown`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L18889) | Keep | Keep |
| [`document_chords_are_the_platforms_and_leave_text_undo_to_the_field`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L18965) | Keep | Keep |
| [`an_open_height_form_keeps_the_copy_workflow_available_on_a_clean_document`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L19062) | Keep | Keep |
| [`document_undo_waits_for_an_open_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L19197) | Port | Window |
| [`a_native_part_is_framed_picked_and_drawn_only_inside_a_shifted_viewport`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L19247) | Keep | Keep |
| [`the_title_marks_unsaved_changes_and_never_names_a_private_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/main.rs#L19492) | Keep | Keep |

### ferritecad-app/src/parameters/tests/native.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_values_edit_the_stored_revolve_angle_from_an_angle_parameter`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/parameters/tests/native.rs#L273) | Keep | Fixture |
| [`native_solver_values_edit_a_distance_constraint_and_publish_its_new_solution`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/parameters/tests/native.rs#L320) | Keep | Fixture |

### ferritecad-app/src/parameters/tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`parameters_park_with_their_tab_and_keep_the_existing_close_undo_checkpoint_guards`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/parameters/tests.rs#L753) | Keep | Keep |

### ferritecad-app/src/sessions/tests/add_chamfer.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_dirty_base_add_distance_history_save_bound_and_branch_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L578) | Keep | Keep |
| [`native_every_corner_adds_its_own_plane_like_the_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L586) | Keep | Keep |
| [`native_constraints_before_or_after_add_and_distance_use_the_solved_plate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L692) | Keep | Keep |
| [`a_refused_add_chamfer_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L770) | Keep | Keep |
| [`mixed_add_chamfer_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L792) | Keep | Keep |
| [`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L1048) | Keep | Keep |
| [`native_compare_real_add_chamfer_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_chamfer.rs#L1142) | Keep | Keep |

### ferritecad-app/src/sessions/tests/add_cut.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_dirty_base_two_adds_edit_history_save_branch_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1020) | Keep | Keep |
| [`native_seventeenth_cut_builds_and_keeps_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1026) | Keep | Keep |
| [`native_constrained_base_accepts_add_after_an_unsaved_constraint_apply`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1064) | Keep | Keep |
| [`a_refused_add_cut_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1136) | Keep | Keep |
| [`mixed_add_cut_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1159) | Keep | Keep |
| [`native_compare_real_add_cut_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_cut.rs#L1169) | Keep | Keep |

### ferritecad-app/src/sessions/tests/add_fillet.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_dirty_base_four_adds_radius_edit_history_save_branch_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L632) | Keep | Keep |
| [`native_add_on_unsaved_constraints_is_measured_on_solved_lines`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L639) | Keep | Keep |
| [`a_refused_add_fillet_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L766) | Keep | Keep |
| [`mixed_add_fillet_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L789) | Keep | Keep |
| [`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L1095) | Keep | Keep |
| [`native_compare_real_add_fillet_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L1201) | Keep | Keep |
| [`a_viewport_pixel_supplies_the_stored_edge_and_a_stale_scene_cannot_add_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/add_fillet.rs#L1212) | Keep | Keep |

### ferritecad-app/src/sessions/tests/analytic.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_a_circle_with_a_fractional_offset_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L542) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_an_annulus_with_unequal_fractional_radii_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L582) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_an_annulus_stored_bore_first_keeps_its_roles_by_curve_uuid`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L596) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_a_circle_moved_and_resized_on_its_canvas_is_applied_and_saved_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L889) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_an_annulus_moved_and_resized_on_its_canvas_keeps_each_wall_under_its_reference`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L910) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_a_circle_dragged_at_a_navigated_scale_is_applied_and_saved_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L990) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_an_annulus_dragged_at_a_navigated_scale_keeps_each_wall_under_its_reference`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L1015) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`a_refused_circle_apply_keeps_the_document_clean`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L1048) | Port | Keep the session refusal/atomicity and preserve its exact text. A form that refuses before submission cannot witness a worker refusal. Use the existing owner transaction test for that layer as well as the new form for the real pre-submit refusal. |
| [`native_constraints_applied_and_removed_through_the_session_keep_the_circle_editor_available`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/analytic.rs#L1080) | Port | Geometry |

### ferritecad-app/src/sessions/tests/angle.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_radial_profile_then_angle_is_one_session_like_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L572) | Port | Angle |
| [`native_axis_closed_profile_then_angle_keeps_axis_and_caps`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L576) | Port | Angle |
| [`native_constraints_then_angle_preserve_stored_approximation_and_cli_cells`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L580) | Port | Angle |
| [`a_refused_angle_apply_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L584) | Keep | Keep |
| [`mixed_angle_apply_needs_no_solver_until_the_profile_is_constrained`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L608) | Port | Angle |
| [`native_compare_real_angle_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/angle.rs#L670) | Keep | Keep |

### ferritecad-app/src/sessions/tests/chamfer.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_height_vertices_then_distances_cross_history_and_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/chamfer.rs#L629) | Keep | Keep |
| [`native_constraints_then_distance_use_solved_sides_and_keep_refused_draft_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/chamfer.rs#L639) | Keep | Keep |
| [`a_refused_distance_apply_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/chamfer.rs#L840) | Keep | Keep |
| [`mixed_distance_apply_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/chamfer.rs#L860) | Keep | Keep |

### ferritecad-app/src/sessions/tests/checkpoints.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`checkpoint_steps_keep_the_picture_are_unsaved_and_undo_like_any_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L81) | Keep | Keep |
| [`a_checkpoint_shares_the_operation_slot_and_a_cancelled_or_late_answer_changes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L119) | Keep | Keep |
| [`the_checkpoint_predicate_says_why_and_waits_for_forms_operations_names_and_the_limit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L210) | Keep | Keep |
| [`a_crash_copy_of_the_window_carries_its_checkpoints`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L286) | Keep | Keep |
| [`a_refused_checkpoint_restore_keeps_the_picture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L331) | Keep | Keep |
| [`mixed_checkpoint_restore_draws_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L410) | Keep | Keep |
| [`native_checkpoints_restore_undo_redo_save_reopen_and_extract_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L571) | Keep | Keep |
| [`native_an_imported_step_checkpoint_needs_no_step_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L710) | Keep | Keep |
| [`native_compare_real_checkpoint_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/checkpoints.rs#L937) | Keep | Keep |

### ferritecad-app/src/sessions/tests/cut.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_height_vertices_and_first_middle_last_of_sixteen_cuts_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L624) | Keep | Keep |
| [`native_floor_refs_transitions_and_refusals_survive_undo_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L628) | Keep | Keep |
| [`a_refused_cut_apply_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L821) | Keep | Keep |
| [`mixed_cut_apply_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L838) | Keep | Keep |
| [`native_compare_real_cut_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L848) | Keep | Keep |
| [`native_open_slot_radius_form_applies_and_survives_history_and_save`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/cut.rs#L999) | Keep | Keep |

### ferritecad-app/src/sessions/tests/datum_planes.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_face_and_offset_sketches_follow_their_bodies_in_the_window_scene`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/datum_planes.rs#L9) | Keep | Keep |

### ferritecad-app/src/sessions/tests/fillet.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_one_and_four_fillets_cross_base_history_and_match_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/fillet.rs#L557) | Keep | Keep |
| [`native_constraints_then_radius_use_solved_rectangle_and_keep_refused_draft_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/fillet.rs#L563) | Keep | Keep |
| [`a_refused_radius_apply_leaves_no_publication`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/fillet.rs#L792) | Keep | Keep |
| [`mixed_radius_apply_uses_occt_without_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/fillet.rs#L818) | Keep | Keep |
| [`native_vertex_form_moves_a_filleted_side_and_keeps_refusals_atomic`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/fillet.rs#L828) | Port | Geometry |

### ferritecad-app/src/sessions/tests/keep_solids.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_face_piece_commands_match_cli_repair_history_and_save`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/keep_solids.rs#L249) | Keep | Keep |
| [`native_piece_selection_refuses_newly_failed_dependents_atomically`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/keep_solids.rs#L445) | Keep | Keep |

### ferritecad-app/src/sessions/tests/live_view.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_window_reuses_other_bodies_on_apply_undo_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/live_view.rs#L59) | Keep | Keep |
| [`stub_open_refuses_before_publishing_a_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/live_view.rs#L176) | Keep | Keep |
| [`native_fifty_apply_undo_cycles_keep_one_revision_and_three_meshes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/live_view.rs#L195) | Keep | Keep |

### ferritecad-app/src/sessions/tests/new_document.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_every_new_document_variant_is_untitled_until_its_first_save_like_the_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L438) | Port | New |
| [`mixed_new_documents_need_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L447) | Port | New |
| [`stub_new_document_is_refused_at_the_picture_and_keeps_the_open_one`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L460) | Keep | Keep |
| [`the_first_save_names_the_document_and_only_then_continues_once`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L492) | Keep | Keep |
| [`native_window_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L843) | Port | New |
| [`native_compare_real_new_document_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/new_document.rs#L921) | Keep | Keep |

### ferritecad-app/src/sessions/tests/owner_panic.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_kernel_panic_answers_and_stops_the_owner`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L6) | Keep | Keep |
| [`a_panicked_owner_refuses_history_but_preserves_saving`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L88) | Keep | Keep |
| [`two_tab_owners_release_only_their_own_revisions`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L163) | Keep | Keep |
| [`a_stale_reply_cannot_discard_another_pending_revision`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L192) | Keep | Keep |
| [`a_second_owner_request_cannot_discard_the_first_candidate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L259) | Keep | Keep |
| [`cancellation_discards_only_candidate_shapes_and_keeps_the_owner_reusable`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L316) | Keep | Keep |
| [`a_switch_reply_releases_its_input_snapshot_before_delivery`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L365) | Keep | Keep |
| [`switching_checks_the_accepted_catalogue_without_touching_the_kernel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/owner_panic.rs#L396) | Keep | Keep |

### ferritecad-app/src/sessions/tests/parameters.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_open_draws_and_saves_a_parameter_driven_extrusion`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/parameters.rs#L6) | Keep | Keep |

### ferritecad-app/src/sessions/tests/recovery.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`recovery_reserves_document_mutation_and_cancel_rejects_its_late_answer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L22) | Keep | Keep |
| [`only_a_version_shown_with_its_picture_is_copied_and_saved_or_undone_states_have_no_copy`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L124) | Keep | Keep |
| [`discard_and_quit_end_the_copy_and_an_exit_nobody_chose_keeps_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L201) | Keep | Keep |
| [`a_recovered_document_is_untitled_named_recovered_and_takes_over_its_record`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L287) | Keep | Keep |
| [`stub_recovery_is_refused_at_the_picture_and_keeps_the_record_and_the_open_document`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L340) | Keep | Keep |
| [`mixed_recovery_draws_without_a_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L368) | Keep | Keep |
| [`recovery_child_entry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L419) | Keep | Fixture |
| [`native_a_killed_window_recovers_named_untitled_and_edited_models_as_accepted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L599) | Keep | Keep |
| [`native_recovery_window_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L924) | Keep | Keep |
| [`native_compare_real_recovery_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/recovery.rs#L1038) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tabs/drafts/canvas_form.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_drawing_the_target_and_the_picks_of_a_form_follow_its_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/canvas_form.rs#L44) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_form_held_on_another_version_loses_its_drawing_and_keeps_its_picks`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/canvas_form.rs#L156) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_its_own_drawing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/canvas_form.rs#L219) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |

### ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`same_document_tabs_keep_literal_forms_and_history_until_their_own_close_succeeds`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L73) | Port | Ownership |
| [`cancelled_close_and_refused_saves_return_the_original_form_and_accepted_model`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L176) | Port | Ownership |
| [`close_decisions_are_once_only_and_address_the_tab_attempt_and_snapshot`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L312) | Port | Ownership |
| [`every_existing_form_family_uses_the_same_move_and_keeps_its_entire_state`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L378) | Port | Ownership |
| [`real_close_buttons_name_unapplied_values_and_parked_fields_cannot_take_typing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L662) | Port | Ownership |
| [`a_refused_hidden_close_upload_keeps_both_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L731) | Port | Ownership |
| [`mixed_close_with_occt_and_no_solver_keeps_the_form_on_cancel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L773) | Port | Ownership |
| [`native_close_cancel_apply_export_save_and_close_matches_cli_and_keeps_b`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L955) | Port | Ownership |
| [`native_compare_real_close_draft_gui_outputs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L975) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`close_request_preserves_foreground_work_and_gesture_holds`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/close_form.rs#L985) | Port | Ownership |

### ferritecad-app/src/sessions/tests/tabs/drafts/dimension_form.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_dimensions_the_text_and_the_prepared_replacement_of_a_form_follow_its_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/dimension_form.rs#L181) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_dimension_form_held_on_another_version_loses_its_labels_and_keeps_its_rows_and_text`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/dimension_form.rs#L260) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_dimension_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/dimension_form.rs#L330) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |

### ferritecad-app/src/sessions/tests/tabs/drafts/marker_form.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_rules_the_selection_and_the_prepared_removal_of_a_form_follow_its_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/marker_form.rs#L108) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_rule_form_held_on_another_version_loses_its_markers_and_keeps_its_rows_selection_and_removal`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/marker_form.rs#L217) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`a_rule_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/marker_form.rs#L296) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |

### ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`accepted_snapshot_tab_identity_stale_actions_and_form_history_are_preserved`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L103) | Port | Ownership |
| [`navigation_open_uses_one_foreground_gesture_and_new_refusal`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L258) | Keep | Keep |
| [`real_widgets_select_uuid_then_send_the_supported_action_and_bound_the_list`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L351) | Port | Ownership |
| [`native_early_cut_navigation_apply_history_save_matches_existing_cli`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L404) | Keep | Keep |
| [`native_fillet_chamfer_revolve_and_sketch_open_their_distinct_existing_routes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L580) | Port | Ownership |
| [`native_compare_real_model_tree_window_outputs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L739) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`history_uses_dependencies_exact_uuids_and_honest_shared_references`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/model_tree.rs#L769) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`open_over_unfinished_forms_keeps_them_with_their_tab_through_every_answer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L297) | Port | Ownership |
| [`new_over_unfinished_forms_sets_them_aside_and_gives_them_back_exactly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L481) | Port | Ownership |
| [`recover_over_an_unfinished_form_keeps_the_record_on_every_refusal`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L756) | Port | Ownership |
| [`stub_open_new_and_recover_beside_a_form_are_refused_at_the_picture_and_keep_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L948) | Port | Ownership |
| [`mixed_open_new_and_recover_beside_forms_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L1001) | Port | Ownership |
| [`native_compare_real_open_new_recover_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L1381) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`native_open_new_recover_beside_forms_then_apply_save_and_compare_with_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L1397) | Port | Ownership |
| [`recover_waiting_for_a_reader_obeys_cancel_late_answers_and_quit_beside_forms_in_two_tabs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_new_recover.rs#L1444) | Port | Ownership |

### ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`open_over_an_unfinished_new_asks_first_and_discards_only_when_told`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L139) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`a_saved_object_form_survives_a_discarded_drawing_exactly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L334) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`a_creation_under_way_is_abandoned_and_its_late_answer_changes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L409) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`a_creation_that_becomes_a_tab_first_is_never_discarded`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L526) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`a_full_window_is_checked_before_anything_is_asked_or_discarded`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L634) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`open_alone_leaves_an_unfinished_new_and_every_other_hold_stays`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L700) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`stub_open_over_a_running_new_is_refused_at_the_picture_and_new_stays_gone`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L831) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`mixed_open_over_a_running_new_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L902) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`native_compare_real_open_during_new_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L1168) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`native_confirmed_open_over_a_running_new_then_late_create_and_back_to_a_match_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/open_over_new.rs#L1185) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |

### ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`late_cancel_returns_all_forms_and_published_save_then_final_quit_cleans_up`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L119) | Port | Ownership |
| [`deferred_discard_and_refused_saves_keep_forms_and_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L222) | Port | Ownership |
| [`form_and_model_answers_address_attempt_tab_and_snapshot_once`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L339) | Port | Ownership |
| [`switch_and_first_last_tabs_refusal_return_previously_confirmed_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L422) | Port | Ownership |
| [`real_quit_widgets_name_loss_back_and_repeat_preserve_literal_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L484) | Port | Ownership |
| [`quit_keeps_foreground_new_export_and_gesture_holds`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L544) | Port | Ownership |
| [`a_refused_quit_switch_upload_keeps_both_original_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L656) | Port | Ownership |
| [`native_cancelled_quit_returns_form_for_apply_export_save_and_final_quit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L705) | Port | Ownership |
| [`mixed_quit_with_occt_without_solver_returns_form_on_cancel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L798) | Port | Ownership |
| [`native_compare_real_quit_draft_gui_outputs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L826) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`every_saved_form_family_returns_whole_after_confirmed_quit_is_aborted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_forms.rs#L851) | Port | Ownership |

### ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`idle_empty_and_sample_new_back_late_abort_and_original_create_are_executable`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L115) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`late_cancel_after_save_or_deferred_discard_returns_new_above_b_and_both_tab_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L268) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`consent_names_quit_and_new_once_and_never_skips_the_dirty_model`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L332) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`save_refusals_dialog_cancel_save_as_and_external_conflict_return_whole_new`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L385) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`scene_switch_refusals_and_first_last_tabs_error_return_all_owners_second_error_exits`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L471) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`running_queued_and_unaccepted_create_hold_quit_and_deliver_their_original_result`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L546) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`real_new_quit_widgets_explain_loss_and_back_and_discard_address_the_actual_owners`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L684) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`stub_real_creation_refusal_returns_idle_new_and_underlying_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L776) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`idle_new_transition_checks_foreground_and_gestures_before_taking_any_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L820) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`new_contour_over_saved_sketch_returns_both_editors_with_functional_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L892) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`native_returned_new_creates_then_publishes_and_matches_cli_sql_uuid_cold_geometry_exports`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L1250) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`mixed_returned_new_creates_with_occt_without_solver_and_keeps_both_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L1267) | Port | Keep the original New generation, dirty answers, refusals, close/quit continuation, stored draft and worker result. The polygon template is separate from the Empty/Sample New owner. Split witnesses where the test exercises both. |
| [`native_compare_real_quit_new_gui_outputs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/quit_new.rs#L1294) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |

### ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`an_open_height_form_blocks_both_parameter_deletion_routes_without_losing_its_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L8) | Keep | Keep |
| [`a_tree_command_rejects_each_stale_address_before_submitting_an_edit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L108) | Keep | Keep |
| [`native_conflicting_sketch_opens_with_named_rules_and_an_unbuilt_dependent`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L173) | Keep | Fixture |
| [`native_picked_face_context_suppresses_the_same_feature_as_the_tree`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L388) | Keep | Keep |
| [`native_tree_deletion_refusal_confirmation_and_single_undo_publish_atomically`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L526) | Keep | Keep |
| [`native_tree_suppression_rebuilds_only_the_suffix_and_undo_restores_the_picture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/tree_commands.rs#L666) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tabs/drafts/turn_form.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_angles_the_selection_the_text_and_the_prepared_replacement_of_a_form_follow_its_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/turn_form.rs#L69) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`an_angle_form_held_on_another_version_loses_its_labels_and_keeps_its_rows_selection_and_text`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/turn_form.rs#L157) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |
| [`an_angle_form_given_back_by_a_cancelled_close_or_an_aborted_quit_keeps_all_of_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts/turn_form.rs#L232) | Port | Use the original supported fixture and the same stored version for a stale answer. Preserve both pictures and comparison of literal form state on each owner. |

### ferritecad-app/src/sessions/tests/tabs/drafts.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`height_drafts_of_two_copies_stay_literal_and_apply_only_to_their_own_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L216) | Keep | Keep |
| [`constraint_drafts_with_one_set_of_ids_keep_picks_changes_and_history_per_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L302) | Port | Ownership |
| [`a_typed_angle_its_pair_and_directions_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L394) | Port | Ownership |
| [`a_picked_pair_and_a_pending_collinear_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L483) | Port | Ownership |
| [`a_typed_diameter_and_the_picked_circle_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L549) | Port | Ownership |
| [`the_canvas_view_its_dragged_numbers_and_their_history_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L645) | Port | Ownership |
| [`the_canvas_zoom_and_pan_follow_their_tab_and_a_held_pan_holds_the_window`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L728) | Port | Preserve the check on both tabs and their gesture hold. The second form can use a real viewport drag or a retained text field. Separately track the retired inside-New canvas gesture. |
| [`running_work_holds_the_window_and_failed_or_foreign_answers_keep_both_drafts`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L828) | Port | Ownership |
| [`close_and_quit_keep_an_open_form_even_over_a_clean_model`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1013) | Keep | Keep |
| [`a_draft_made_on_another_version_comes_back_held_and_is_never_applied`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1107) | Keep | Keep |
| [`the_same_control_in_two_tabs_shares_no_focus_or_text`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1172) | Keep | Keep |
| [`vertex_drafts_of_two_copies_keep_their_own_text_and_draft_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1246) | Port | Ownership |
| [`object_snap_and_its_snapped_draft_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1300) | Port | Keep isolation, reset of ephemeral hover and restore on tab switch. Retire the legacy-only From object anchor or typed Length. Use viewport selection, drag, grid and guide state for the shared assertions. |
| [`guides_a_typed_length_and_the_guided_draft_follow_their_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1362) | Port | Keep isolation, reset of ephemeral hover and restore on tab switch. Retire the legacy-only From object anchor or typed Length. Use viewport selection, drag, grid and guide state for the shared assertions. |
| [`the_anchor_mode_follows_its_tab_and_the_resting_anchor_follows_nobody`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1418) | Port | Keep isolation, reset of ephemeral hover and restore on tab switch. Retire the legacy-only From object anchor or typed Length. Use viewport selection, drag, grid and guide state for the shared assertions. |
| [`a_refused_switch_upload_keeps_both_drafts`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1477) | Port | Ownership |
| [`mixed_drafts_switch_and_apply_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1520) | Keep | Keep |
| [`native_drafts_of_two_models_apply_after_returning_and_save_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1561) | Port | Ownership |
| [`native_replace_length_drafts_of_two_copies_apply_to_their_own_tab_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L1769) | Port | Ownership |
| [`native_compare_real_tab_drafts_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L2034) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`native_tab_drafts_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/drafts.rs#L2047) | Port | Keep the shape, saved object/curve identity, cache/cold evidence and negative controls of the comparator. Keep the matching real window producer. A GUI output cannot be substituted with the CLI reference. |

### ferritecad-app/src/sessions/tests/tabs/restore.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`quit_keeps_the_saved_files_only_when_the_window_really_ends`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L220) | Keep | Keep |
| [`an_empty_end_clears_the_offer_and_a_list_that_cannot_be_kept_stops_quit_once`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L483) | Keep | Keep |
| [`reopen_restores_saved_files_beside_open_tabs_in_order_and_shows_the_last_shown`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L564) | Keep | Keep |
| [`cancel_late_answers_and_a_full_window_never_revive_or_mix_a_reopen`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L708) | Keep | Keep |
| [`stub_reopen_is_refused_at_the_picture_and_keeps_the_open_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L848) | Keep | Keep |
| [`mixed_reopen_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L903) | Keep | Keep |
| [`reopen_waits_for_a_gesture_and_is_offered_only_with_a_list`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L934) | Keep | Keep |
| [`native_compare_real_reopen_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L1237) | Keep | Keep |
| [`native_reopen_saved_tabs_then_apply_save_and_compare_with_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L1253) | Keep | Keep |
| [`launch_journal_child`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L1289) | Keep | Keep |
| [`the_window_route_journals_its_start_a_real_error_a_panic_and_its_end`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/restore.rs#L1341) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tabs/workspace.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`long_messages_and_long_lists_cannot_move_the_viewport_or_paint_over_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L304) | Keep | Keep |
| [`the_same_layout_at_two_pixels_per_point_is_the_same_rectangle_twice_as_large`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L392) | Keep | Keep |
| [`an_empty_window_and_an_empty_document_stay_controllable`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L426) | Port | Tree |
| [`panels_keep_their_presses_and_the_viewport_keeps_its_own`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L550) | Keep | Keep |
| [`dragging_a_panel_moves_the_viewport_and_what_was_asked_in_the_old_one_is_dropped`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L630) | Keep | Keep |
| [`each_tab_keeps_its_camera_across_a_layout_change_and_is_fitted_to_the_present_viewport`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L679) | Keep | Keep |
| [`a_question_that_must_be_answered_first_leaves_the_layout_and_the_viewport_alone`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L752) | Keep | Keep |
| [`a_form_keeps_its_focus_and_text_when_the_groups_above_it_come_and_go`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs/workspace.rs#L801) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tabs.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`two_copies_of_one_document_are_two_tabs_with_their_own_history_saves_and_checkpoints`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L404) | Keep | Keep |
| [`opening_a_file_open_by_any_name_shows_its_tab_and_copies_are_new_tabs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L496) | Keep | Keep |
| [`save_as_onto_another_tabs_file_is_refused_and_the_existing_guards_still_hold`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L544) | Keep | Keep |
| [`closing_asks_only_about_unsaved_tabs_and_cancel_or_a_failed_save_keeps_the_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L589) | Keep | Keep |
| [`quit_asks_every_unsaved_tab_in_turn_and_cancel_part_way_closes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L694) | Keep | Keep |
| [`late_cancelled_or_foreign_answers_change_no_tab_and_release_no_slot`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L815) | Keep | Keep |
| [`each_tab_keeps_its_own_files_and_crash_copy_and_closing_frees_only_its_own`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L941) | Keep | Keep |
| [`the_ninth_document_is_refused_before_anything_changes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1073) | Keep | Keep |
| [`a_refused_switch_upload_keeps_the_shown_tab`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1111) | Keep | Keep |
| [`native_two_tabs_alternate_applies_undo_restore_exports_and_saves_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1201) | Port | Ownership |
| [`mixed_tabs_switch_and_apply_with_occt_and_no_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1411) | Keep | Keep |
| [`native_compare_real_tabs_gui_artifacts_with_negative_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1747) | Keep | Keep the real-file comparator and all negative controls. Port the window artifact producer in this group, using the original CLI reference and file checks. |
| [`native_tabs_scenario_on_session_files_passes_the_comparator_and_its_controls`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1760) | Port | Keep the shape, saved object/curve identity, cache/cold evidence and negative controls of the comparator. Keep the matching real window producer. A GUI output cannot be substituted with the CLI reference. |
| [`quit_waits_for_open_forms_and_foreground_work_even_when_tabs_are_clean`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1842) | Keep | Keep |
| [`a_cancelled_switch_reply_is_ignored_while_an_apply_runs`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1934) | Keep | Keep |
| [`a_cancelled_switch_keeps_its_input_alive_until_its_worker_finishes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tabs.rs#L1985) | Keep | Keep |

### ferritecad-app/src/sessions/tests/tree_operations.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_suppressed_document_opens_and_undo_redo_preserve_the_visible_status`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/tree_operations.rs#L68) | Keep | Keep |

### ferritecad-app/src/sessions/tests/up_to_end.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_referenced_end_draws_the_boss_height_and_cut_form_explains_editing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions/tests/up_to_end.rs#L6) | Keep | Keep |

### ferritecad-app/src/sessions.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`apply_delivers_the_step_picture_without_spawning_a_scene_worker`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1470) | Keep | Keep |
| [`a_version_is_current_only_when_its_picture_is_shown`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1514) | Keep | Keep |
| [`a_failed_stale_or_cancelled_apply_changes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1552) | Keep | Keep |
| [`a_cancelled_edit_that_still_finished_is_dropped_and_leaves_no_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1584) | Keep | Keep |
| [`a_picture_that_cannot_be_shown_drops_the_version_and_keeps_the_last_good_state`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1619) | Keep | Keep |
| [`a_picture_that_was_ready_when_cancel_was_pressed_is_not_accepted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1641) | Keep | Keep |
| [`a_no_op_apply_is_reported_and_adds_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1690) | Keep | Keep |
| [`undo_and_redo_move_only_when_their_picture_is_shown`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1709) | Keep | Keep |
| [`save_moves_the_checkpoint_only_when_the_file_was_published`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1760) | Keep | Keep |
| [`replacing_the_document_asks_only_when_something_would_be_lost`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1823) | Keep | Keep |
| [`the_name_is_the_users_never_the_private_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1853) | Keep | Keep |
| [`an_old_editor_gets_the_users_folder_and_name_and_its_copy_outlives_the_old_session`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1875) | Keep | Keep |
| [`a_save_still_running_when_another_document_is_adopted_never_moves_its_checkpoint`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L1987) | Keep | Keep |
| [`an_export_keeps_its_working_copy_alive_without_blocking_the_window`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2046) | Keep | Keep |
| [`a_public_export_into_the_working_folder_is_refused_before_anything_is_written`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2115) | Keep | Keep |
| [`a_vertex_apply_is_one_document_step_and_a_stale_or_unchanged_form_is_not`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2226) | Keep | Fixture |
| [`a_vertex_apply_cancelled_after_it_was_computed_is_not_accepted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2306) | Keep | Fixture |
| [`stopping_joins_the_worker_and_removes_every_private_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2331) | Keep | Keep |
| [`an_apply_beside_an_imported_step_part_shows_both`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L2490) | Keep | Keep |
| [`native_vertices_of_an_ordinary_offset_polygon_are_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3001) | Port | Geometry |
| [`native_a_vertex_moved_from_another_anchor_is_applied_undone_exported_and_saved_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3014) | Port | Geometry |
| [`native_vertices_of_a_rectangle_under_a_chamfer_keep_the_chamfer_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3052) | Port | Geometry |
| [`native_a_refused_vertex_edit_changes_nothing_and_constrained_lines_remain_drawable`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3063) | Port | Keep the session refusal/atomicity and preserve its exact text. A form that refuses before submission cannot witness a worker refusal. Use the existing owner transaction test for that layer as well as the new form for the real pre-submit refusal. |
| [`native_an_ordinary_plate_is_applied_undone_exported_and_saved_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3146) | Keep | Keep |
| [`native_a_chamfered_plate_with_constraints_keeps_them_byte_for_byte`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3245) | Port | Rule |
| [`constraint_comparison_keeps_the_sketch_row_metadata`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3455) | Keep | Keep the stored-rule row metadata comparator independent of the form. |
| [`native_replace_length_of_a_dimensioned_plate_under_a_chamfer_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3692) | Port | Rule |
| [`native_directed_angle_of_a_dart_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3745) | Port | Port once-only publication and independent CLI comparison. Reversed vector endpoints cannot be selected independently when adding Angle. Preserve the exact rule using an existing CLI-built saved-rule fixture where it tests subsequent editing. Lead decision needed if creation of that exact rule is itself asserted. |
| [`native_collinear_of_a_u_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3823) | Port | Rule |
| [`native_radius_and_fixed_centre_of_a_circle_are_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3898) | Port | Rule |
| [`native_rules_chosen_on_the_canvas_of_a_constrained_rectangle_are_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3938) | Port | Rule |
| [`native_a_radius_and_a_fixed_centre_chosen_on_the_canvas_of_a_solved_circle_are_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L3969) | Port | Rule |
| [`native_a_saved_length_chosen_by_its_label_and_replaced_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4003) | Port | Rule |
| [`native_a_saved_radius_chosen_by_its_label_and_replaced_by_a_diameter_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4040) | Port | Rule |
| [`native_a_saved_angle_chosen_by_its_label_and_replaced_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4082) | Port | Rule |
| [`native_a_saved_equal_length_chosen_by_its_marker_and_removed_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4140) | Port | Rule |
| [`native_a_saved_fixed_centre_chosen_by_its_marker_and_removed_is_applied_like_the_command_line`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4194) | Port | Rule |
| [`native_a_solver_conflict_or_a_chamfer_that_no_longer_fits_changes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4244) | Port | Keep the session refusal/atomicity and preserve its exact text. A form that refuses before submission cannot witness a worker refusal. Use the existing owner transaction test for that layer as well as the new form for the real pre-submit refusal. |
| [`without_the_solver_a_constraints_apply_is_refused_and_nothing_changes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4361) | Port | Retain the owner-level declined apply and absence of publication. The viewport refuses an unavailable solver before submitting. Keep both layer-specific witnesses and disclose that interaction difference. |
| [`a_refused_apply_leaves_the_document_clean`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4409) | Keep | Keep |
| [`without_the_solver_a_constrained_plate_cannot_supply_an_apply_baseline`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4471) | Keep | Keep |
| [`native_open_draws_from_the_sessions_own_copy_and_a_failed_open_leaves_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4522) | Keep | Keep |
| [`a_cancelled_save_changes_nothing_and_says_so`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sessions.rs#L4570) | Keep | Keep |

### ferritecad-app/src/sketch/accepted_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_nearest_part_within_nine_logical_points_wins_and_the_boundary_is_exact`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L57) | Delete | Retire |
| [`an_exact_tie_goes_to_the_stored_order_and_the_order_decides_not_the_ids`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L95) | Delete | Retire |
| [`an_end_is_start_or_end_by_the_stored_line_and_never_by_the_side_of_the_screen`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L123) | Delete | Retire |
| [`a_circle_is_one_curve_whose_ring_and_centre_are_told_apart`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L140) | Delete | Retire |
| [`degenerate_and_non_numeric_shapes_are_never_picked`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L169) | Delete | Retire |
| [`a_part_outside_the_visible_canvas_cannot_be_picked`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L193) | Delete | Retire |
| [`a_drawing_is_shown_only_for_the_curves_the_form_stores_in_the_order_it_stores_them`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L255) | Delete | Retire |
| [`only_a_line_polygon_or_one_circle_is_a_profile_the_canvas_covers`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L308) | Delete | Retire |
| [`a_coordinate_that_cannot_be_drawn_keeps_the_whole_drawing_away`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L347) | Delete | Retire |
| [`fit_shows_every_live_curve_inside_the_canvas_with_finite_numbers`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/accepted_tests.rs#L384) | Delete | Retire |

### ferritecad-app/src/sketch/analytic.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`fields_that_cannot_be_drawn_are_named_and_numbers_that_can_are_kept_as_they_are`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic.rs#L607) | Delete | Retire |
| [`crowded_handles_go_to_the_nearest_and_an_exact_tie_to_centre_outer_bore`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic.rs#L666) | Delete | Retire |
| [`a_fit_shows_the_larger_positive_radius_centred_with_the_line_canvas_margins`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic.rs#L715) | Delete | Retire |

### ferritecad-app/src/sketch/analytic_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_new_circle_moves_its_centre_and_radius_by_real_events_one_undo_step_each`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L262) | Delete | Retire |
| [`a_new_annulus_changes_each_radius_alone_and_every_profile_keeps_its_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L356) | Delete | Retire |
| [`saved_forms_take_a_gesture_as_one_confirmed_step_and_apply_exactly_its_numbers`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L436) | Delete | Retire |
| [`typed_unconfirmed_text_comes_back_exactly_with_the_first_undo_after_a_drag`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L533) | Delete | Retire |
| [`gestures_in_one_frame_keep_their_order_and_a_no_op_keeps_the_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L590) | Delete | Retire |
| [`escape_lost_focus_and_a_vanished_pointer_restore_the_exact_strings_and_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L700) | Delete | Retire |
| [`busy_queued_covered_and_outside_presses_start_nothing_and_held_fields_take_no_text`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L753) | Port | Window |
| [`crowded_handles_at_a_small_scale_follow_the_pick_rule_through_real_presses`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L952) | Delete | Retire |
| [`one_and_two_pixels_per_point_give_the_same_strings_and_pick_radius`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L1027) | Delete | Retire |
| [`a_refused_profile_stays_a_visible_draft_and_text_that_cannot_be_drawn_draws_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L1055) | Delete | Retire |
| [`the_view_fits_on_opening_and_on_fit_only_and_the_form_stays_within_768_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/analytic_tests.rs#L1129) | Delete | Retire |

### ferritecad-app/src/sketch/anchor.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_pointer_acquires_inside_twelve_points_and_holds_until_beyond_twenty_four`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L191) | Delete | Retire |
| [`a_nearer_candidate_replaces_the_held_one_only_by_more_than_one_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L226) | Delete | Retire |
| [`the_first_anchor_is_the_object_snap_choice_and_stays_stable_for_any_layout`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L279) | Delete | Retire |
| [`a_held_anchor_that_is_gone_hidden_or_unmeasurable_is_released`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L323) | Delete | Retire |
| [`the_same_pointer_in_points_holds_the_same_anchor_at_any_scale`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L355) | Delete | Retire |
| [`words_name_the_real_anchor_and_the_vertex_the_gesture_changes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor.rs#L374) | Delete | Retire |

### ferritecad-app/src/sketch/anchor_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_mode_sits_in_the_guides_row_needs_guides_and_changes_only_editor_state`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L92) | Delete | Retire |
| [`a_new_point_is_added_from_a_shown_endpoint_midpoint_or_crossing_reached_by_real_events`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L148) | Delete | Retire |
| [`a_saved_vertex_moves_from_another_anchor_without_switching_the_selection`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L334) | Delete | Retire |
| [`a_held_anchor_is_released_at_twice_the_aperture_and_replaced_only_by_a_clearly_nearer_one`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L473) | Delete | Retire |
| [`a_long_gesture_keeps_the_pressed_anchor_even_past_other_points_and_twice_the_aperture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L520) | Delete | Retire |
| [`an_object_point_beats_a_guide_from_the_anchor_and_a_length_decides_what_it_keeps`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L569) | Delete | Retire |
| [`a_refused_length_moves_no_saved_vertex_even_beside_an_object_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L668) | Delete | Retire |
| [`escape_lost_focus_and_a_vanished_pointer_restore_the_strings_and_a_release_outside_ends_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L694) | Delete | Retire |
| [`packed_events_keep_their_order_and_the_settings_belong_to_the_press`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L783) | Delete | Retire |
| [`off_busy_covered_and_unreadable_canvases_offer_no_anchor_and_typed_text_stays_exact`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L872) | Delete | Retire |
| [`a_result_that_breaks_the_contour_stays_a_draft_and_restore_keeps_the_mode`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L1049) | Delete | Retire |
| [`one_and_two_pixels_per_point_and_other_canvas_scales_hold_the_same_anchor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L1086) | Delete | Retire |
| [`the_form_keeps_apply_and_save_in_reach_with_the_mode_on_at_768_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L1163) | Delete | Retire |
| [`a_new_contour_closed_from_an_anchor_hands_the_create_job_exactly_the_polygon_it_shows`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/anchor_tests.rs#L1211) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |

### ferritecad-app/src/sketch/dimension_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`saved_dimensions_are_the_whole_line_lengths_and_the_one_circle_radius_in_curve_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/dimension_tests.rs#L121) | Delete | Retire |
| [`a_label_states_the_target_and_beside_it_only_what_is_prepared_in_its_place`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/dimension_tests.rs#L255) | Delete | Retire |
| [`a_label_stands_beside_its_curve_whichever_way_it_is_stored_and_inside_the_canvas`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/dimension_tests.rs#L386) | Delete | Retire |
| [`a_label_is_none_when_its_curve_cannot_be_drawn_or_its_anchor_is_off_the_canvas`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/dimension_tests.rs#L487) | Delete | Retire |
| [`labels_that_overlap_are_reached_in_stored_order_and_a_label_is_exactly_its_box`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/dimension_tests.rs#L556) | Delete | The placed/hit-box sorter is legacy code. The viewport uses its own projected pick. Retire the legacy ordering witness. |

### ferritecad-app/src/sketch/drag_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`pointer_drag_is_one_precise_undo_step_and_selects_only_its_vertex`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L109) | Delete | Retire |
| [`pointer_escape_focus_and_outside_release_have_explicit_outcomes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L179) | Delete | Retire |
| [`pointer_hit_ties_free_add_and_invalid_preview_use_existing_draft_policy`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L233) | Delete | Retire |
| [`snap_change_after_a_coalesced_drag_does_not_rewrite_that_gesture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L277) | Delete | Retire |
| [`snap_rounds_values_below_half_a_step_toward_the_nearer_grid_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L317) | Port | Port platform-independent half-step/finite-number checks of shared grid rounding. Retire only string formatting and legacy canvas placement. |
| [`snap_large_finite_displacements_remain_refused_without_overflow`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L326) | Port | Port platform-independent half-step/finite-number checks of shared grid rounding. Retire only string formatting and legacy canvas placement. |
| [`snap_preserves_invalid_coordinate_refusals_without_panicking`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L348) | Port | Port platform-independent half-step/finite-number checks of shared grid rounding. Retire only string formatting and legacy canvas placement. |
| [`snap_step_rounds_pointer_input_without_changing_fields_or_off_precision`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L375) | Port | Port platform-independent half-step/finite-number checks of shared grid rounding. Retire only string formatting and legacy canvas placement. |
| [`native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L836) | Port | Exercise curve movement via the surviving viewport tool and retain every stored UUID, SQL cell, geometry, publication and export comparison. The retired typed guide length or midpoint snap itself is a separate gap. |
| [`coalesced_pointer_events_keep_press_target_and_history_stays_bounded`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L841) | Delete | Retire |
| [`table_scroll_and_numeric_escape_do_not_start_canvas_gestures`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L883) | Delete | Retire |
| [`single_frame_drag_uses_press_and_release_even_after_later_hover`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L928) | Delete | Retire |
| [`release_then_next_press_in_one_frame_keeps_the_next_drag_active`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L972) | Delete | Retire |
| [`several_gestures_in_one_frame_keep_changed_checkpoints_in_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1005) | Delete | Retire |
| [`cancellation_before_release_in_one_frame_does_not_publish_a_gesture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1047) | Delete | Retire |
| [`completed_pointer_gesture_does_not_edit_through_an_overlapping_window`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1079) | Delete | Retire |
| [`native_solid_revolve_widgets_drag_worker_and_cli_create_and_edit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1135) | Port | Geometry |
| [`native_partial_revolve_profile_widgets_drag_worker_and_cli_edit_one_copy`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1441) | Port | Geometry |
| [`restore_saved_vertices_stays_out_of_one_drag`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1733) | Delete | Retire |
| [`chamfer_base_drag_is_one_undo_step_and_moves_no_neighbour`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/drag_tests.rs#L1836) | Delete | Retire |

### ferritecad-app/src/sketch/guide.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`axes_and_diagonals_are_in_the_xy_plane_with_y_up`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L533) | Keep | Keep |
| [`a_guide_is_held_within_the_aperture_across_it_and_not_beyond`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L577) | Keep | Keep |
| [`distance_is_to_the_guide_not_to_its_origin_so_far_pointers_still_hold_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L597) | Keep | Keep |
| [`the_anchor_itself_a_zero_vector_and_non_finite_numbers_offer_no_guide`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L607) | Port | Assert the same heading, aperture, hysteresis and scale at guide::acquire. Replace the removed canvas resolve adapter in the fixture. |
| [`a_held_guide_is_kept_until_another_is_nearer_by_more_than_one_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L663) | Keep | Keep |
| [`a_first_choice_among_near_equal_guides_is_axis_then_distance_then_table_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L714) | Keep | Keep |
| [`the_same_pointer_in_points_picks_the_same_guide_at_any_scale`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L746) | Keep | Keep |
| [`object_over_guide_over_free_point_and_the_grid_is_not_applied_to_a_guide`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L765) | Delete | Retire |
| [`a_typed_length_is_exact_and_beats_the_pointer_the_grid_and_a_foreign_object`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L794) | Delete | Retire |
| [`a_length_that_is_not_a_positive_finite_number_is_refused_never_zero`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L856) | Delete | Retire |
| [`axis_guides_keep_the_anchor_text_and_labels_say_what_the_length_measures`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide.rs#L900) | Delete | Retire |

### ferritecad-app/src/sketch/guide_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_guides_row_follows_the_snap_row_and_changes_only_editor_state`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L130) | Delete | Retire |
| [`a_new_point_follows_a_shown_guide_from_the_last_vertex_with_the_anchor_drawn_apart`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L187) | Delete | Retire |
| [`polar_diagonals_and_the_first_point_and_what_the_canvas_does_not_show`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L256) | Delete | Retire |
| [`a_typed_length_places_the_exact_point_and_one_that_cannot_be_used_places_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L319) | Delete | Retire |
| [`an_object_point_beats_a_guide_and_only_a_matching_length_keeps_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L390) | Delete | Retire |
| [`the_grid_step_is_not_applied_to_a_guided_point_but_still_rules_a_free_one`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L461) | Delete | Retire |
| [`a_saved_vertex_drag_measures_from_where_it_was_pressed_and_keeps_that_anchor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L477) | Delete | Retire |
| [`a_new_contour_drag_is_a_guided_one_step_and_escape_focus_and_pointer_loss_restore_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L579) | Delete | Retire |
| [`packed_events_keep_their_order_and_the_settings_belong_to_the_press`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L642) | Delete | Retire |
| [`off_busy_and_covered_canvases_offer_no_guide`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L731) | Delete | Retire |
| [`one_and_two_pixels_per_point_and_other_canvas_scales_hold_the_same_guide`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L862) | Delete | Retire |
| [`typed_text_stays_exact_and_restore_saved_vertices_keeps_the_editor_settings`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L930) | Delete | Retire |
| [`a_guided_result_that_breaks_the_contour_stays_a_visible_draft_and_is_refused_as_before`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L970) | Delete | Retire |
| [`native_guided_length_drag_worker_and_cli_agree_on_ids_sql_and_geometry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/guide_tests.rs#L1041) | Port | Exercise curve movement via the surviving viewport tool and retain every stored UUID, SQL cell, geometry, publication and export comparison. The retired typed guide length or midpoint snap itself is a separate gap. |

### ferritecad-app/src/sketch/marker_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`saved_markers_are_the_seven_families_with_their_exact_references_in_rule_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/marker_tests.rs#L107) | Delete | Retire |
| [`a_marker_says_its_kind_in_ascii_and_a_prepared_removal_beside_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/marker_tests.rs#L320) | Delete | Retire |
| [`a_marker_stands_where_its_references_are_and_is_absent_when_they_cannot_be_drawn`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/marker_tests.rs#L381) | Delete | Retire |
| [`markers_overlap_in_saved_order_and_a_marker_is_exactly_its_box`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/marker_tests.rs#L549) | Delete | The placed/hit-box sorter is legacy code. The viewport uses its own projected pick. Retire the legacy ordering witness. |
| [`a_marker_carries_its_references_whole_and_reaches_nothing_when_nothing_stands`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/marker_tests.rs#L632) | Delete | Retire |

### ferritecad-app/src/sketch/navigation.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`zoom_keeps_the_document_point_under_the_cursor_at_every_scale_and_display`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation.rs#L378) | Delete | Retire |
| [`the_scale_stops_at_its_limits_without_moving_the_point_under_the_cursor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation.rs#L430) | Delete | Retire |
| [`nothing_that_is_not_a_finite_number_reaches_the_view`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation.rs#L474) | Delete | Retire |
| [`a_pan_holds_the_grabbed_point_under_the_pointer_and_the_corner_stays_finite`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation.rs#L542) | Delete | Retire |

### ferritecad-app/src/sketch/navigation_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`wheel_and_pinch_zoom_about_the_cursor_on_every_canvas_and_edit_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L412) | Delete | Retire |
| [`a_pan_follows_the_pointer_holds_the_window_and_edits_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L490) | Delete | Retire |
| [`a_pan_that_does_not_move_is_no_click_and_places_or_selects_nothing_on_the_line_canvas`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L636) | Delete | Retire |
| [`a_held_edit_gesture_freezes_the_view_and_what_the_press_froze`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L677) | Delete | Retire |
| [`navigation_reaches_only_the_visible_uncovered_canvas_of_a_form_that_is_free`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L710) | Delete | Retire |
| [`a_pan_ends_without_a_trace_by_release_outside_escape_focus_pointer_gone_or_a_busy_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L854) | Port | Window |
| [`events_of_one_frame_keep_their_order_and_gestures_read_the_view_the_person_saw`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L940) | Delete | Retire |
| [`zooming_in_makes_close_vertices_short_midpoints_and_a_tiny_bore_pickable_in_logical_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L1132) | Delete | Retire |
| [`fit_is_the_only_way_back_and_typing_dragging_and_invalid_text_never_refit`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L1245) | Delete | Retire |
| [`the_hint_names_the_gestures_and_says_when_the_view_is_locked`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L1333) | Delete | Retire |
| [`each_profile_keeps_its_own_view_and_cancelling_the_draft_forgets_all_of_them`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L1383) | Delete | Retire |
| [`navigation_over_a_canvas_never_reaches_the_3d_camera_and_a_viewport_drag_still_crosses_the_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/navigation_tests.rs#L1462) | Port | View |

### ferritecad-app/src/sketch/object_snap.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`kinds_rank_intersection_endpoint_midpoint_and_name_themselves`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L338) | Delete | Retire |
| [`crowded_short_segments_resolve_either_side_of_two_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L356) | Delete | Retire |
| [`a_gap_of_exactly_one_point_gives_the_nearest_and_less_gives_the_kind`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L379) | Delete | Retire |
| [`ties_follow_kind_then_distance_then_stored_order_in_any_layout`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L396) | Delete | Retire |
| [`the_aperture_is_inclusive_and_candidates_beyond_it_never_compete`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L434) | Delete | Retire |
| [`the_choice_depends_on_logical_points_not_on_canvas_scale`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L461) | Delete | Retire |
| [`crossings_are_of_finite_segments_not_of_their_extensions`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L488) | Delete | Retire |
| [`degenerate_parallel_overlapping_and_shared_cases_make_no_crossing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L522) | Delete | Retire |
| [`a_window_makes_exactly_the_unrestricted_candidates_inside_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L618) | Delete | Retire |
| [`reversing_the_drawing_gives_the_same_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L660) | Delete | Retire |
| [`a_drag_keeps_the_press_candidates_without_the_vertex_and_what_sits_on_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L685) | Delete | Retire |
| [`the_largest_drawing_has_a_bounded_finite_set_and_a_cheap_pointer_query`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L737) | Delete | Retire |
| [`coordinates_from_objects_are_exact_and_unrounded`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap.rs#L770) | Delete | Retire |

### ferritecad-app/src/sketch/object_snap_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_switch_sits_beside_the_grid_step_and_changes_only_editor_state`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L182) | Delete | Retire |
| [`new_points_take_crossing_midpoint_and_endpoint_with_a_marker_and_a_name`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L217) | Delete | Retire |
| [`an_object_point_beats_the_grid_is_never_rounded_and_typed_text_stays_exact`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L273) | Delete | Retire |
| [`a_drag_lands_on_the_frozen_incident_midpoint_never_on_itself_and_is_one_undo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L336) | Delete | Retire |
| [`a_snap_that_breaks_the_contour_stays_a_draft_and_cancelling_leaves_history_alone`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L429) | Delete | Retire |
| [`packed_events_keep_order_and_a_switch_in_the_frame_changes_only_the_next_gesture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L491) | Delete | Retire |
| [`off_busy_and_covered_canvases_offer_no_snap`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L562) | Delete | Retire |
| [`one_and_two_pixels_per_point_and_other_canvas_scales_pick_the_same_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L656) | Delete | Retire |
| [`only_what_the_canvas_shows_is_offered`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L721) | Delete | Retire |
| [`saved_vertices_drag_to_a_midpoint_and_ask_for_the_request_typed_numbers_make`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L738) | Delete | Retire |
| [`native_object_snap_drag_worker_and_cli_agree_on_ids_sql_and_geometry`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/object_snap_tests.rs#L870) | Port | Exercise curve movement via the surviving viewport tool and retain every stored UUID, SQL cell, geometry, publication and export comparison. The retired typed guide length or midpoint snap itself is a separate gap. |

### ferritecad-app/src/sketch/tests/analytic_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_open_forms_apply_through_the_windows_own_predicate_and_keep_their_drafts`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L223) | Port | Window |
| [`nothing_else_running_or_open_lets_the_command_start`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L319) | Port | Window |
| [`the_stored_numbers_offer_no_apply_and_a_draft_round_trip_keeps_its_redo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L390) | Port | Window |
| [`the_draft_confirmation_is_not_the_apply_and_both_are_named_honestly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L450) | Delete | Retire |
| [`with_unsaved_changes_save_copy_is_withheld_with_words_and_apply_is_not`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L500) | Port | The old form refuses Save a copy on a dirty source. Use the surviving Revolve angle or height copy workflow for that guard and its exact text. Finishing a sketch followed by Save As changes the open model. |
| [`a_new_accepted_version_ends_the_drafts_and_keeps_what_the_window_said`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L547) | Port | Window |
| [`the_forms_open_on_a_document_with_unsaved_changes_only_when_the_window_says_so`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L620) | Port | Window |
| [`the_documents_last_outcome_is_readable_inside_the_open_form`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L672) | Port | Window |
| [`while_a_worker_is_busy_the_forms_start_no_second_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/analytic_apply.rs#L729) | Port | Window |

### ferritecad-app/src/sketch/tests/angle_apply.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`angle_apply_uses_current_fields_and_confirmation_is_only_draft_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/angle_apply.rs#L127) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |
| [`angle_apply_busy_guards_first_prove_a_changed_request_is_available`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/angle_apply.rs#L204) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |
| [`angle_apply_requires_its_own_form_and_session_and_supported_kind`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/angle_apply.rs#L320) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |
| [`angle_range_refusal_status_and_accepted_scene_dismissal_are_visible`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/angle_apply.rs#L373) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |
| [`angle_form_opens_dirty_and_withholds_only_the_copy_workflow`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/angle_apply.rs#L444) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |

### ferritecad-app/src/sketch/tests/new_over_forms.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`new_opened_over_a_floating_form_shares_no_focus_or_text_with_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/new_over_forms.rs#L71) | Port | Ownership |

### ferritecad-app/src/sketch/tests/open_over_new.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`the_question_beside_new_keeps_its_text_and_focus_and_both_buttons_answer`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/tests/open_over_new.rs#L148) | Port | Ownership |

### ferritecad-app/src/sketch/turn_label_tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`saved_angles_are_the_directed_angle_rules_between_two_lines_with_their_exact_vectors_in_rule_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/turn_label_tests.rs#L120) | Delete | Retire |
| [`a_label_states_the_signed_target_the_order_and_the_unit_and_never_reads_as_another_value`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/turn_label_tests.rs#L263) | Delete | Retire |
| [`the_label_of_a_pair_stands_midway_between_its_lines_and_is_finite_for_every_relative_position`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/turn_label_tests.rs#L423) | Delete | Retire |
| [`labels_of_curves_and_pairs_overlap_in_saved_order_and_a_label_is_exactly_its_box`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/turn_label_tests.rs#L536) | Delete | Retire |
| [`an_angle_typed_back_from_its_display_is_the_saved_angle_for_the_probes_and_the_gap_is_told`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch/turn_label_tests.rs#L609) | Port | Keep editable_degrees round-trip witnesses. Move them to the viewport dimension field. Discard the retired label hitbox or old exact UI sentence only. |

### ferritecad-app/src/sketch.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`floating_forms_in_two_tabs_do_not_share_keyboard_focus`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L3711) | Port | Window |
| [`apply_vertices_is_offered_when_it_would_work_and_keeps_the_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L3869) | Port | Window |
| [`the_open_sketch_form_can_apply_through_the_windows_busy_guard`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L3926) | Port | Window |
| [`with_unsaved_changes_the_copy_workflow_is_withheld_but_apply_is_not`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L3997) | Port | The old form refuses Save a copy on a dirty source. Use the surviving Revolve angle or height copy workflow for that guard and its exact text. Finishing a sketch followed by Save As changes the open model. |
| [`a_new_accepted_version_ends_the_forms_about_the_old_one_and_only_those`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4032) | Port | Window |
| [`draft_undo_redo_closure_and_shared_validity`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4055) | Delete | Retire |
| [`a_dirty_document_can_open_a_new_drawing_without_enabling_copy_workflows`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4098) | Port | Keep underlying dirty saved-object session and accepted picture. Open ordinary New… Empty over the form. The old drawing-specific clean-copy controls have their own gap. |
| [`draft_widgets_submit_exact_mouse_coordinates_with_explicit_closure`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4313) | Delete | Retire |
| [`circle_widgets_submit_exact_numbers_and_keep_the_polygon_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4481) | Delete | Retire |
| [`annulus_widgets_submit_exact_numbers_and_keep_the_other_drafts`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4577) | Delete | Retire |
| [`native_annulus_draft_and_cli_publish_equivalent_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4717) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |
| [`revolve_draft_widgets_name_the_axis_and_defer_contact_to_the_kernel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4868) | Delete | Retire |
| [`native_revolve_draft_worker_and_cli_publish_equivalent_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L4902) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |
| [`partial_revolve_draft_widgets_state_the_angle_and_refuse_others`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5053) | Delete | Retire |
| [`native_partial_revolve_draft_worker_and_cli_publish_equivalent_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5090) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |
| [`circle_edit_widgets_change_only_the_two_numbers_and_keep_the_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5213) | Delete | Retire |
| [`native_revolve_profile_edit_widgets_worker_and_cli_publish_one_copy`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5366) | Port | Geometry |
| [`native_revolve_angle_edit_widgets_worker_and_cli_publish_one_copy`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5694) | Keep | Keep the existing partial-turn route, confirmation, copy and angle-unit conversion. Move its fixture if necessary. |
| [`native_circle_edit_worker_and_cli_publish_equivalent_copies`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L5959) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`annulus_edit_widgets_change_only_the_three_numbers_and_keep_the_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6176) | Delete | Retire |
| [`native_annulus_edit_worker_and_cli_publish_equivalent_copies`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6383) | Port | Keep the previously asserted geometry, UUIDs, cold output and artifacts. Exact unconstrained circle radius changes on a saved circle have no viewport input. Decide the stimulus before removing the producer. A persistent Radius changes stored rows. |
| [`native_circle_draft_and_cli_publish_equivalent_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6562) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |
| [`native_sketch_draft_and_cli_publish_equivalent_models`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6640) | Port | Keep model acceptance and CLI equivalence. New… Empty is one publication and sketch/sweep have separate versions. Exact one-publication drawing-template semantics require a lead decision. |
| [`native_saved_sketch_draft_and_cli_preserve_same_model`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6817) | Port | Geometry |
| [`native_cut_base_widgets_worker_cli_preserve_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L6824) | Port | Geometry |
| [`native_two_fillet_base_sketch_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7120) | Port | Geometry |
| [`native_fillet_base_sketch_widgets_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7201) | Port | Geometry |
| [`native_chamfer_base_sketch_widgets_worker_and_cli_publish_the_same_part`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7319) | Port | Geometry |
| [`restore_saved_vertices_is_one_widget_step_and_publishes_nothing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7457) | Delete | Retire |
| [`restore_saved_vertices_on_a_revolve_and_a_cut_base`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7771) | Delete | Retire |
| [`restore_saved_vertices_is_absent_from_other_forms`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch.rs#L7932) | Delete | Retire |

### ferritecad-app/src/sketch_mode/drag.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`a_refused_sample_retains_the_last_solution_and_the_next_sample_recovers`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/drag.rs#L326) | Keep | Keep |

### ferritecad-app/src/sketch_mode/snapping.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`every_grid_step_rounds_halves_away_from_zero_in_sketch_coordinates`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/snapping.rs#L80) | Keep | Keep |
| [`an_object_beats_a_guide_and_a_guide_keeps_the_exact_anchor_coordinate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/snapping.rs#L94) | Keep | Keep |
| [`guide_scale_tracks_foreshortening_at_its_anchor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/snapping.rs#L125) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/buttons.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`each_button_requires_its_exact_selection_and_a_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/buttons.rs#L9) | Keep | Keep |
| [`opposite_lines_measure_positive_pi_even_when_the_first_line_is_selected_last`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/buttons.rs#L97) | Keep | Keep |
| [`line_end_and_circle_centre_take_precedence_over_projected_curves`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/buttons.rs#L118) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`rectangle_widgets_constrain_the_drawing_in_one_transaction_and_extrude_it`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L80) | Keep | Keep |
| [`saved_distance_changes_the_width_to_eighty_through_the_value_field`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L133) | Keep | Keep |
| [`radius_and_diameter_start_at_the_measured_size_and_history_never_solves`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L226) | Keep | Keep |
| [`angle_is_measured_in_pick_order_and_set_accepts_degrees`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L346) | Keep | Keep |
| [`deleting_a_saved_viewport_marker_removes_only_its_rule`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L402) | Keep | Keep |
| [`deleting_a_constrained_line_solves_the_remaining_drawing_once`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L425) | Keep | Keep |
| [`a_saved_parameter_dimension_is_read_only_and_keeps_its_source`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L456) | Keep | Keep |
| [`contradictory_distance_keeps_every_curve_rule_and_history_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L521) | Keep | Keep |
| [`unavailable_solver_leaves_constraint_controls_disabled_and_drawing_usable`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/constraint_widgets.rs#L612) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/dragging.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`fixed_end_stays_and_free_end_slides_on_its_horizontal_through_finish`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L110) | Keep | Keep |
| [`a_dragged_rectangle_keeps_its_corners_and_one_history_step_through_finish`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L149) | Keep | Keep |
| [`an_unconstrained_handle_moves_alone_and_does_not_snap_to_itself`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L227) | Keep | Keep |
| [`a_handle_click_and_idle_hover_select_without_starting_a_solver`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L252) | Keep | Keep |
| [`escape_and_focus_loss_restore_the_press_without_touching_history`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L271) | Keep | Keep |
| [`empty_space_and_curve_bodies_keep_the_orbit_gesture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L292) | Keep | Keep |
| [`constrained_drag_in_a_build_without_the_solver_keeps_the_drawing`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L306) | Keep | Keep |
| [`a_line_uses_the_grid_then_its_latest_vertex_and_an_exact_object_snap`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L340) | Keep | Keep |
| [`consecutive_drags_released_between_frames_keep_both_steps_in_order`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L373) | Keep | Keep |
| [`returning_the_handle_to_its_start_creates_no_history_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L395) | Keep | Keep |
| [`a_contradictory_drawing_reports_the_same_named_rules_at_drag_start_as_on_rebuild`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L414) | Keep | Keep |
| [`a_coincident_corner_follows_pointer_steps_shorter_than_the_snap_aperture`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L457) | Keep | Keep |
| [`delete_during_a_drag_keeps_the_dragged_curve_and_one_history_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/dragging.rs#L490) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/gpu.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`gpu_mode_hides_only_its_saved_drawing_and_uploads_once_per_transition`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/gpu.rs#L22) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/prediction.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`releasing_a_fixed_point_preserves_the_preview_after_finish`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L33) | Keep | Keep |
| [`release_propagates_to_a_coincident_line_end_without_writing_an_independent_curve`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L38) | Keep | Keep |
| [`release_preserves_a_remaining_horizontal_between_two_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L44) | Keep | Keep |
| [`inconsistent_second_prediction_refuses_without_a_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L223) | Keep | Keep |
| [`prediction_writes_only_beyond_the_display_tolerance_per_coordinate`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L248) | Keep | Keep |
| [`new_curve_precedes_its_constraint_and_cancelled_rule_addition_is_omitted`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L284) | Keep | Keep |
| [`returning_a_dimension_to_its_original_value_keeps_its_saved_spelling`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L325) | Keep | Keep |
| [`finished_seed_uses_saved_coordinates_new_draft_curves_and_final_rule_values`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/prediction.rs#L359) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests/window.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`widgets_finish_empty_difference_and_cancel_leave_version_and_file_unchanged`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L158) | Keep | Keep |
| [`widgets_circle_point_construction_selection_delete_and_draft_history_own_keys`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L200) | Keep | Keep |
| [`widgets_escape_and_edge_on_use_exact_messages_and_keep_navigation_gestures`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L261) | Keep | Keep |
| [`native_new_sketch_rectangle_widgets_commit_one_version_and_one_document_undo`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L314) | Keep | Keep |
| [`native_new_sketch_rectangle_with_reverse_side_reads_the_same_closed_profile`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L318) | Keep | Keep |
| [`native_refusal_keeps_draft_history_full_message_and_accepted_file`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L444) | Keep | Keep |
| [`native_solved_seed_is_not_written_and_deleted_line_lists_its_constraint_explicitly`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L489) | Keep | Keep |
| [`mode_uses_existing_availability_tab_parking_close_and_quit_questions`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L584) | Keep | Keep |
| [`catalogue_offers_mode_only_for_accepted_drawings_and_new_sketch_only_for_planes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L669) | Keep | Keep |
| [`existing_arcs_can_be_selected_toggled_deleted_and_undone_through_widgets`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L706) | Keep | Keep |
| [`mode_consumes_hover_and_readonly_clicks_without_entity_requests_or_draft_changes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L743) | Keep | Keep |
| [`entry_refuses_absent_objects_planes_and_mismatched_accepted_versions`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L793) | Keep | Keep |
| [`native_finish_of_new_empty_sketch_reuses_selected_plane_without_an_extra_datum`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L822) | Keep | Keep |
| [`native_failed_sketch_does_not_offer_or_open_the_viewport_editor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests/window.rs#L840) | Keep | Keep |

### ferritecad-app/src/sketch_mode/tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`pixel_rays_intersect_all_three_standard_planes_in_both_projections`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L51) | Keep | Keep |
| [`arbitrary_plane_look_keeps_the_target_and_distance_and_refuses_invalid_axes`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L80) | Keep | Keep |
| [`edge_on_and_empty_viewports_have_no_plane_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L106) | Keep | Keep |
| [`hit_testing_uses_the_same_projected_line_circle_arc_and_point`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L138) | Keep | Keep |
| [`snapping_chooses_the_nearest_exact_coordinate_without_rounding`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L177) | Keep | Keep |
| [`a_curve_deleted_from_the_draft_is_no_longer_a_snap_target`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L196) | Keep | Keep |
| [`snap_targets_include_arc_ends_and_all_centres_and_points`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L216) | Keep | Keep |
| [`polyline_preserves_shared_endpoint_bits_and_closes_only_on_the_start_click`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L246) | Keep | Keep |
| [`current_endpoint_and_escape_end_a_polyline_without_a_curve`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L281) | Keep | Keep |
| [`circle_zero_radius_and_escape_add_nothing_but_a_completed_circle_is_one_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L298) | Keep | Keep |
| [`selection_toggles_and_one_delete_or_construction_is_one_history_step`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L322) | Keep | Keep |
| [`request_omits_added_then_removed_and_double_toggled_curves`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L347) | Keep | Keep |
| [`request_preserves_untouched_stored_coordinates_in_a_solved_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L363) | Keep | Keep |
| [`request_explicitly_removes_constraints_before_curves_then_adds_and_flags`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L406) | Keep | Keep |
| [`new_empty_sketch_creates_only_its_missing_standard_plane_and_sketch`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L455) | Keep | Keep |
| [`standard_plane_reuse_is_by_exact_placement_and_sketch_name_is_first_free`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L482) | Keep | Keep |
| [`deleting_a_circle_explicitly_removes_its_whole_curve_radius_constraint`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sketch_mode/tests.rs#L510) | Keep | Keep |

### ferritecad-app/src/sweeps/tests/window.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`native_rectangle_drawn_with_widgets_becomes_one_body_and_one_undo_keeps_the_sketch`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L82) | Keep | Keep |
| [`native_add_blind_widgets_use_the_selected_existing_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L169) | Keep | Keep |
| [`native_cut_through_all_widgets_add_faces_to_the_complete_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L203) | Keep | Keep |
| [`native_symmetric_and_reversed_widgets_store_both_intents`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L231) | Keep | Keep |
| [`native_revolve_full_turn_widgets_build_about_sketch_y`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L308) | Keep | Keep |
| [`native_revolve_partial_widgets_store_ninety_degrees_as_radians`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L312) | Keep | Keep |
| [`native_revolve_widgets_accept_a_construction_line_axis`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L316) | Keep | Keep |
| [`open_polyline_keeps_both_commands_disabled_with_the_accepted_complete_reason`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L321) | Port | Keep the full no-profile reason and disabled sweep checks. Remove only the retired Constraints variant from the expected menu. |
| [`native_axis_crossing_refusal_preserves_the_form_version_and_file_until_cancel`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L391) | Keep | Keep |
| [`native_add_at_rollback_names_and_uses_the_shown_predecessor`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L448) | Keep | Keep |
| [`sweep_form_uses_the_existing_command_tab_close_and_quit_lifecycle`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L506) | Keep | Keep |
| [`no_body_disables_existing_body_operations_and_invalid_number_keeps_draft`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L619) | Keep | Keep |
| [`sweep_entry_requires_the_accepted_drawing_and_an_available_source`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L653) | Keep | Keep |
| [`native_line_axis_labels_use_the_solved_drawing_without_rewriting_stored_coordinates`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L677) | Keep | Keep |
| [`apply_requires_idle_and_enabled_owner_permission`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L749) | Keep | Keep |
| [`native_failed_sketch_has_no_drawing_or_sweep_commands`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests/window.rs#L804) | Keep | Keep |

### ferritecad-app/src/sweeps/tests.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`numbered_names_share_all_object_names_and_fill_each_prefix_gap`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L20) | Keep | Keep |
| [`new_body_is_one_add_body_with_two_new_identities_and_free_names`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L47) | Keep | Keep |
| [`add_cut_intersect_each_send_one_feature_to_the_selected_body`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L82) | Keep | Keep |
| [`extrusion_ends_and_reverse_are_saved_exactly_and_unused_distance_is_not_parsed`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L134) | Keep | Keep |
| [`revolve_axes_full_turn_and_partial_radians_are_exact`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L174) | Keep | Keep |
| [`partial_angle_uses_saved_angle_bounds_and_complete_errors`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L207) | Keep | Keep |
| [`line_axes_include_construction_in_document_order_with_exact_accepted_coordinates`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L251) | Keep | Keep |
| [`invalid_distance_and_absent_body_refuse_without_creating_a_request`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L284) | Keep | Keep |
| [`unnamed_sketch_refusal_uses_the_same_object_label_and_preserves_every_reason_word`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-app/src/sweeps/tests.rs#L305) | Keep | Keep |

### ferritecad-ui/src/panels.rs

| Test at the base revision | Disposition | Method or specific invariant |
| --- | --- | --- |
| [`new_controls_follow_creation_availability`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-ui/src/panels.rs#L3794) | Keep | Keep |
| [`new_form_create_asks_nothing_while_the_window_says_it_cannot`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-ui/src/panels.rs#L3826) | Keep | Keep |
| [`new_form_shows_millimetres_and_reports_its_real_buttons`](https://github.com/gesriot/ferrite-cad/blob/9a0b0fbbe5f7215b7db944b8b4a16e5c56d05dd9/crates/ferritecad-ui/src/panels.rs#L3877) | Keep | Keep |
