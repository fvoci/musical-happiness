# fvoci-server 포팅 노트

원본: `server/api`, `server/app`, `server/auth`, `server/ws/server.go`, `server/web`, `server/server`, `server/main`, `server/services/{auth,config,permissions/localpermissions,webhook,scheduler}`. 원본의 team은 워크스페이스다.

## 오라클에서 확인한 사실 (2026-11-10)

포팅 방향을 정할 때 원본 서버를 직접 실행해 확인한 것들이다.

| 확인한 것 | 결과 |
|---|---|
| 등록·로그인 | 첫 사용자는 토큰 없이 등록 가능. 이후 토큰 없는 등록은 401 `no sign-up token and user(s) already exist`. 잘못된 로그인 401 `incorrect login`, `type`이 `normal`이 아니면 400 `invalid login type` |
| 세션 없는 요청 | 401 `no session token` |
| `X-Requested-With` 누락 | 400 `checkCSRFToken FAILED` |
| `GET /teams/{아무 ID}` | 루트 팀(`id: "0"`)과 `signupToken`을 돌려준다 |
| 빈 목록 | 블록·구독·사용자 목록은 `[]` |
| `POST /boards`에 `type` 누락 | 400 `invalid-board-type` |
| 블록 생성 응답의 `createAt` | 요청 값을 그대로 돌려주지만 DB에는 현재 시각이 저장된다 |
| `GET /boards/{id}/blocks?type=h1` | 200 `[]` (모르는 타입도 오류가 아니다) |
| 없는 API 경로, 있는 경로에 다른 메서드 | 둘 다 200과 `index.html` (JSON 404/405가 아니다) |
| `/static`, `/staticx/y` | 404 |
| `/static/` | 디렉터리 목록 HTML |
| 없는 정적 파일 | 404 `404 page not found` |
| single-user 모드의 사용자 경로 | `GET /users/me`는 가상의 `single-user`를 돌려주지만 `POST /users ["single-user"]`와 `GET /users/single-user`는 404 `{user} not found`(가상 사용자를 만든 뒤 DB에서 다시 찾기 때문). `POST /teams/0/users ["single-user"]`는 200. `/login`·`/register`·`/logout`·`changepassword`는 401 `not permitted in single-user mode` |
| 원본 bcrypt 해시 | `$2a$10$…` 형식. 포트의 `compare_password`로 검증됨(테스트에 실제 해시 사용) |

## 구조와 API 매핑

원본의 패키지 경계를 모듈로 옮겼다. 핸들러 구조체(`api.API`)와 앱 구조체(`app.App`)는 `App` 하나로 합쳤고, 원본이 인터페이스로 갈아 끼우던 것(권한 서비스, 저장소, WebSocket 어댑터)은 standalone 구현 하나뿐이라 구체 타입으로 둔다.

| 원본 | Rust | 비고 |
|---|---|---|
| `server/server/server.go` `Server`, `New`, `Start`, `Shutdown` | `Server::{new, router, serve, shutdown}` (`src/lib.rs`) | 리스너는 호출자가 넘긴다(테스트는 포트 0). 종료는 future로 받는다 |
| `server/main/main.go` | `src/main.rs` (`fvoci-server`) | 플래그 `--config`, `--port`, `--dbtype`, `--dbconfig`. `--single-user`는 S6에서 없앴다. `--print-openapi`는 설정을 읽지 않고 계약 문서를 찍고 끝난다(S6 O절) |
| (없음) | `api::contract` (`src/api/contract.rs`, `contract/{params,bodies}.rs`) | API의 계약 `contracts/openapi.json`을 라우트 정의에서 만든다. 핸들러의 `#[utoipa::path]`와 문서 주석이 계약의 글이므로, 핸들러의 문서 주석은 클라이언트가 읽는 글로 쓴다(루트 `PORTING.md` S6 O절) |
| `server/services/config` | `config::Configuration` | `read`, `parse`, `check` |
| `server/app/app.go` `App` | `app::App` | `Arc` 하나를 감싼 값. `config()`, `store()`, `ws()`, `files()`, `webhook()` |
| `app.blockChangeNotifier` (`utils.CallbackQueue`) | `App::spawn_background`, `App::finish_background` | 큐 대신 추적되는 tokio 태스크. 종료 때 10초까지 기다린다 |
| `server/auth/auth.go` `Auth` | `auth::Authenticator` | `get_session`, `is_valid_read_token` |
| `server/services/auth/request_parser.go` | `auth::token::parse_auth_token` | |
| `server/services/auth/password.go` | `auth::password` | |
| `server/services/permissions/localpermissions` | `permissions::grant`(순수 함수), 사실은 `App::board_access`·`App::actor` | 권한은 enum `BoardPermission`, 답은 `Grant` |
| `server/api/api.go` `sessionRequired`, `attachSession` | 추출기 `api::RequiredSession`, `api::OptionalSession` | |
| `server/api/api.go` `requireCSRFToken` | `api::router`의 route layer | |
| `server/api/api.go` `errorResponse`, `server/model/errors.go` | `error::AppError` + `IntoResponse` | |
| `server/api/{auth,users,teams,system,config}.go` | `src/api/{auth,users,workspace,system,config}.rs` | 파일 하나가 모듈 하나 |
| `server/app/{auth,user,teams,clientConfig,server_metadata}.go` | `src/app/{auth,users,workspaces}.rs`, `src/app/mod.rs` | |
| `server/ws/server.go` | `ws::Hub` | 연결마다 보내기 큐(512개)를 두고, 못 따라오는 연결은 끊는다 |
| `server/web/webserver.go` | `web::WebRoot`, `Server::router`의 fallback | |
| `server/services/webhook` | `webhook::Webhook` | |
| `server/api/blocks.go`, `server/app/blocks.go` | `src/api/blocks.rs`, `src/app/blocks.rs` | `disableNotify bool` → `app::ChangeNotification::{Send, Suppress}`. `App::{blocks, blocks_for_board, block, insert_block, insert_blocks, patch_block, patch_blocks, delete_block, undelete_block, duplicate_block, last_block_history_entry}` |
| `server/api/{boards,members,boards_and_blocks}.go`, `server/app/{boards,boards_and_blocks}.go` | `src/api/{boards,members,boards_and_blocks}.rs`, `src/app/{boards,boards_and_blocks}.rs` | `addMember bool` → 메서드 쌍 `create_board` / `create_board_with_admin`(boards-and-blocks도 같다). `App::{board, boards_for_user_and_workspace, patch_board, delete_board, undelete_board, duplicate_board, members_for_board, add_member_to_board, update_board_member, delete_board_member, create_boards_and_blocks, patch_boards_and_blocks, delete_boards_and_blocks}` |
| `server/api/{cards,content_blocks,subscriptions}.go`, `server/app/{cards,content_blocks,subscriptions}.go` | `src/api/{cards,subscriptions}.rs`, `src/app/{cards,subscriptions}.rs` | S7에서 콘텐츠 블록 이동(`content_blocks`)을 걷어 냈다: 카드의 내용이 본문이다(`docs/s6-design.md` 5.7). 그때까지 `where` 문자열 → `app::MovePosition::{Before, After}`. `App::{create_card, cards_for_board, patch_card, card, create_subscription, delete_subscription, subscriptions}(S6까지 `move_content_block`도)` |
| `server/api/{files,archive,templates,onboarding}.go`, `server/app/{files,export,import,templates,onboarding}.go` | `src/api/{files,archive,templates,onboarding}.rs`, `src/app/{files,export,import,templates,onboarding}.rs` | `App::{save_file, get_file_info, get_file_path, open_stored_file, get_file_reader, move_file, copy_and_update_card_files, export_archive, import_archive, init_templates, template_boards, prepare_onboarding_tour}`. 내장 템플릿은 이 크레이트의 `assets/templates/`에만 있고(웹앱의 메모리 구현도 그 파일을 읽는다) `include_str!`로 바이너리에 넣는다(S7, 설계 3.10). ZIP은 `zip` 크레이트(순수 Rust 압축 백엔드) |
| `server/api/{categories,sharing,search}.go`, `server/app/{category,category_boards,sharing}.go` | `src/api/{categories,sharing,search}.rs`, `src/app/{category,category_boards,sharing,search}.rs` | `visible bool` → `BoardVisibility`. `App::{category, create_category, update_category, delete_category, reorder_categories, user_category_boards, add_update_user_category_board, reorder_category_boards, set_board_visibility, sharing, upsert_sharing, search_boards_for_user, search_boards_for_user_in_workspace}` |
| `server/api/admin.go`, `api.go`의 `adminRequired`, `server.go`의 로컬 소켓 | `src/api/admin.rs`, `api::admin_router`, `lib.rs`의 `admin_socket` | Unix 소켓으로 들어온 연결만 통과 |
| `server/services/scheduler` | `scheduler::RecurringTask` | 세션 정리(10분)에 쓴다. 1회성 작업(`CreateTask`)은 쓰는 곳이 없어 옮기지 않았다 |
| `server/api/{channels,statistics,compliance}.go` | `src/api/{channels,statistics,compliance}.rs` | standalone의 거절 응답만 |
| `server/web/webserver.go`의 TLS | `Server::with_tls_files`, `lib.rs`의 `tls` | `tokio-rustls` |
| `server/api/api.go` `panicHandler`, 요청 로그 | `lib.rs`의 `request_log` | |
| 파일 저장소(호스트 애플리케이션의 `filestore`) | `storage::FileBackend::{Local, S3}` | |

## 테스트 매핑

원본 테스트 하나(또는 서브테스트 하나)에 Rust 테스트 하나가 대응한다. `tests/api/`는 프로세스 안에서 서버를 띄우고 HTTP로 호출한다(원본 `integrationtests`의 `TestHelper`에 대응). mock 저장소를 쓰던 `server/app`의 테스트는 실제 저장소로 옮겼다.

### 핵심부

| 원본 | Rust | 비고 |
|---|---|---|
| `localpermissions_test.go` `TestHasPermissionToTeam`, `TestHasPermissionToBoard` | `permissions::table` (역할 × 범위 × 할 일의 표) | "empty input", "nonexistent member"는 API 테스트에서 확인 |
| `ws/server_test.go` `TestTeamSubscription` | `ws::tests::workspace_subscription`, `ws::tests::workspace_subscription_subscribe_users_to_workspace_retrieve_by_user` | |
| `ws/server_test.go` `TestBlocksSubscription` | `ws::tests::blocks_subscription` | |
| `ws/server_test.go` `TestGetUserIDForTokenInSingleUserMode` | `ws::tests::get_user_id_for_token_in_single_user_mode` | |
| `ws/plugin_adapter_test.go`, `ws/helpers_test.go` | — | 플러그인 어댑터 전용 |
| `services/auth/password_test.go` | `auth::password::tests::*` | |
| `services/auth/request_parser_test.go` | `auth::token::tests::*` | |
| `auth/auth_test.go` `TestGetSession` | `users::get_me_not_login_yet`(토큰 없음), `users::session_token_locations`(잘못된 토큰, 맞는 토큰) | mock 대신 실제 세션 |
| `auth/auth_test.go` `TestIsValidReadToken` | `websocket::block_subscription_needs_the_read_token` | 원본 테스트 본문은 비어 있다 |
| `web/webserver_test.go` | `tests/web.rs` | |
| `services/webhook/webhook_test.go` `TestClientUpdateNotify` | `tests/webhook.rs::client_update_notify` | |
| `api/system_test.go` `TestHello` | `system::hello_returns_hello_on_success` | |
| `api/system_test.go` `TestPing` (2건) | `system::ping_returns_metadata_on_success`, `system::ping_sets_sku_to_personal_desktop_when_in_single_user_mode` | "plugin mode → suite"는 해당 모드가 없다 |
| `app/server_metadata_test.go` `TestGetServerMetadata` | `system::ping_returns_metadata_on_success` | |
| `app/clientConfig_test.go` `TestGetClientConfig` | `system::get_client_config` | 텔레메트리 값은 고정 |
| `app/auth_test.go` `TestLogin` | `users::login_cases` | |
| `app/auth_test.go` `TestGetUser` | `users::app_get_user_cases` | |
| `app/auth_test.go` `TestRegisterUser` | `users::register_user_cases` | |
| `app/auth_test.go` `TestUpdateUserPassword` | `users::app_update_user_password_cases` | |
| `app/auth_test.go` `TestChangePassword` | `users::app_change_password_cases` | |
| `app/teams_test.go` `TestGetRootTeam` | `users::app_get_root_workspace` | "upsert 실패" 케이스는 mock으로만 만들 수 있어 해당 없음 |
| `app/teams_test.go` `TestGetTeam`, `TestTeamOperations` | — | mock 호출 전달만 확인한다. 저장소 쪽 동작은 `fvoci-store`의 워크스페이스 테스트가 덮는다 |
| `app/user_test.go` `TestSearchUsers` | `users::users_list_and_profiles` | 팀/시스템 관리자와 채널 케이스는 호스트 애플리케이션 전용 |
| `integrationtests/user_test.go` `TestUserRegister` | `users::user_register` | |
| `integrationtests/user_test.go` `TestUserLogin` (2건) | `users::user_login_with_nonexist_user`, `users::user_login_with_registered_user` | |
| `integrationtests/user_test.go` `TestGetMe` (2건) | `users::get_me_not_login_yet`, `users::get_me_logged_in` | |
| `integrationtests/user_test.go` `TestGetUser` (2건) | `users::get_user` | |
| `integrationtests/user_test.go` `TestGetUserList` | — | 플러그인 모드 헬퍼로만 돈다 |
| `integrationtests/user_test.go` `TestUserChangePassword` | `users::user_change_password` | |
| `integrationtests/user_test.go` `TestTeamUploadFile` | 파일 영역 참고 | |

원본에 없고 포트에서 추가한 테스트: `users::change_password_of_another_user_is_forbidden`(보안 수정), `users::logout_ends_the_session`, `users::user_preferences`, `users::workspaces`, `users::single_user_mode`, `system::api_requests_need_the_requested_with_header`, `system::api_responses_are_not_cacheable`, `system::unknown_paths_get_the_web_app`, `system::base_path_prefixes_every_route`, `websocket::*`(실제 연결로 인증·구독·수신자 선정·ping 확인), `tests/storage.rs`, 설정 파서 단위 테스트.

### 관리자 소켓, 스케줄러, 거절 라우트, TLS, 오류 분류

| 원본 | Rust | 비고 |
|---|---|---|
| `scheduler_test.go` `TestCreateRecurringTask` | `scheduler::tests::create_recurring_task` | tokio의 멈춘 시계 사용 |
| `scheduler_test.go` `TestCancelTask` | `scheduler::tests::cancel_task` | |
| `scheduler_test.go` `TestCreateTask` | — | 1회성 작업은 옮기지 않았다 |
| `api/api_test.go` `TestErrorResponse` (케이스별) | `error::tests::error_response_<케이스>_should_be_a_<코드>_code` | `ErrViewsLimitReached`, `ErrPatchUpdatesLimitedCards`, `ErrInsufficientLicense`는 포트에 대응 오류가 없어 해당 없음 |
| `integrationtests/compliance_test.go` (3개 함수, 16케이스) | `admin::get_boards_for_compliance_is_refused_in_standalone_mode` 등 3건 | 플러그인 모드 전용. "비로그인 거절", "권한 없는 사용자 거절"만 standalone 응답으로 확인 |
| `utils/callbackqueue_test.go` | — | 큐를 tokio 태스크 추적으로 바꿔 대응 코드가 없다 |

추가한 테스트: `admin::socket::*`(비밀번호 설정, 사용자 이름 디코딩, 빈 비밀번호 거절, 72바이트 초과 거절, 없는 사용자·잘못된 본문 500, 소켓에는 관리자 라우트만, TCP로는 닿지 않음, 소켓 권한 0600과 종료 시 삭제, 이전 실행의 소켓 교체, 기본은 닫힘), `admin::use_ssl_*` 3건(TLS 서빙, 실패한 핸드셰이크, 인증서 없으면 평문. 마지막 것은 S6에서 "인증서 없으면 시작 거부" 2건과 "`usessl`이 꺼져 있으면 무관" 1건으로 바뀌었다), `get_channel_is_not_implemented_in_standalone_mode`, `statistics_are_not_implemented_in_standalone_mode`, `standalone_refusals_are_the_same_in_single_user_mode`, `sessions_are_cleaned_up_after_the_longer_of_31_days_and_their_lifetime`, `lib.rs`의 요청 로그·클라이언트 주소·패닉 처리 단위 테스트.

### 블록

테스트의 보드는 저장소로 직접 만든다(`create_board_in_store`). 원본의 서브테스트는 상태를 공유하므로 Rust 테스트는 앞 단계를 각자 다시 수행한다.

| 원본 | Rust | 비고 |
|---|---|---|
| `integrationtests/blocks_test.go` `TestGetBlocks` | `blocks::get_blocks` | |
| `TestPostBlock` (3건) | `blocks::post_block_create_a_single_block`, `…_create_a_couple_of_blocks_in_the_same_call`, `…_update_a_block_should_not_be_possible_through_the_insert_endpoint` | |
| `TestPatchBlock` (3건) | `blocks::patch_block_patch_a_block_basic_field`, `…_custom_fields`, `…_to_remove_custom_fields` | |
| `TestDeleteBlock` (2건) | `blocks::delete_block_create_a_block`, `blocks::delete_block_delete_a_block` | |
| `TestUndeleteBlock` (4건) | `blocks::undelete_block_create_a_block`, `…_delete_a_block`, `…_undelete_a_block`, `…_try_to_undelete_a_block_without_permissions` | |
| `app/blocks_test.go` `TestInsertBlock` (2건) | `blocks::app_insert_block_success_scenario`, `…_error_scenario` | mock 대신 실제 실패(제목 초과, 없는 보드) |
| `TestPatchBlocks` success, error | `blocks::app_patch_blocks_success_scenario`, `…_error_scenario` | |
| `TestPatchBlocks` "cloud limit error scenario" | — | 호스팅판의 카드 한도(원본에서도 skip) |
| `TestDeleteBlock` (2건) | `blocks::app_delete_block_success_scenario`, `…_error_scenario` | |
| `TestUndeleteBlock` success | `blocks::app_undelete_block_success_scenario` | |
| `TestUndeleteBlock` error | — | mock으로만 만들 수 있는 저장소 실패 |
| `TestInsertBlocks` success, error | `blocks::app_insert_blocks_success_scenario`, `…_error_scenario` | |
| `TestInsertBlocks` 뷰 한도 3건 | — | 호스팅판의 뷰 한도(원본에서도 skip) |

추가한 테스트 24건: `get_blocks_single_user_needs_the_token`, `get_blocks_permissions`, `get_blocks_of_templates`, `get_blocks_with_a_read_token`, `get_blocks_filters`, `get_blocks_by_block_id`, `post_blocks_*` 6건, `patch_block_*`·`patch_blocks_batch_*` 4건, `delete_block_*`·`undelete_block_*`·`duplicate_block_*` 5건, `block_changes_are_broadcast`, `block_changes_call_the_webhook`, `app_undelete_block_that_never_existed`.


### 카테고리(사이드바), 공유, 검색

mock 저장소를 쓰던 `server/app`의 테스트는 실제 저장소로 옮겼다. 원본의 비공개 함수(`moveBoardsToDefaultCategory`, `verifyNewCategoriesMatchExisting`)는 라우트를 통해 확인한다.

| 원본 | Rust (`tests/api/sidebar.rs`, `sharing.rs`, `search.rs`) | 비고 |
|---|---|---|
| `integrationtests/sidebar_test.go` `TestSidebar` | `sidebar::sidebar` | 저장소로 보드를 만드는 같은 흐름이 `sidebar_with_boards_from_the_store` |
| `TestHideUnhideBoard` | `sidebar::hide_unhide_board` | |
| `app/category_test.go` `TestCreateCategory` (2건) | `create_category_base_case`, `create_category_creating_invalid_category` | 모르는 타입은 enum으로 만들 수 없어 `create_category_refuses_invalid_requests`에서 HTTP로 확인 |
| `TestUpdateCategory` (6건) | `update_category_base_case`, `…_updating_invalid_category`, `…_trying_to_update_someone_elses_category`, `…_trying_to_update_some_other_workspaces_category`, `…_should_not_be_allowed_to_rename_system_category`, `…_should_be_allowed_to_collapse_and_expand_any_category_type` | |
| `TestDeleteCategory` (3건) | `delete_category_base_case`, `…_trying_to_delete_already_deleted_category`, `…_trying_to_delete_system_category` | |
| `TestMoveBoardsToDefaultCategory` (2건) | `move_boards_to_default_category_when_default_category_already_exists`, `…_doesnt_already_exists` | 삭제를 통해 |
| `TestReorderCategories` (2건) | `reorder_categories_base_case`, `…_not_specifying_all_categories_should_fail` | |
| `TestVerifyNewCategoriesMatchExisting` (2건) | `verify_new_categories_match_existing_base_case`, `…_different_category_counts` | |
| `app/category_boards_test.go` `TestGetUserCategoryBoards` (3건) | `get_user_category_boards_user_had_no_default_category_and_had_boards`, `…_but_had_no_boards`, `…_user_already_had_a_default_boards_category_with_boards_in_it` | |
| `TestCreateBoardsCategory` (4건) | `create_boards_category_user_doesnt_have_any_boards_implicit_or_explicit`, `…_has_implicit_access_to_some_board`, `…_has_explicit_access_to_some_board`, `…_has_both_implicit_and_explicit_access_to_some_board` | synthetic 멤버십은 호스트 애플리케이션의 것이라, 멤버가 아닌 공개 보드로 대신했다 |
| `TestReorderCategoryBoards` (2건) | `reorder_category_boards_base_case`, `…_not_specifying_all_boards` | |
| `integrationtests/sharing_test.go` `TestSharing` (4건) | `sharing::sharing_an_unauthenticated_client_should_not_be_able_to_get_a_sharing`, `sharing_check_no_initial_sharing`, `sharing_post_sharing_config_false`, `sharing_post_sharing_config_true` | 원본은 테스트 중간에 설정을 바꾼다. 포트는 케이스마다 서버를 따로 띄운다 |
| `app/sharing_test.go` `TestGetSharing` 성공·not found | `get_sharing_should_get_a_sharing_successfully`, `get_sharing_should_return_a_not_found_error` | |
| `TestGetSharing` "should fail", `TestUpsertSharing` "should fail" | — | mock 저장소의 실패만 확인 |
| `TestUpsertSharing` 성공 | `upsert_sharing_should_success_to_upsert_sharing` | |
| `integrationtests/board_test.go` `TestSearchBoards` (2건) | `search::search_boards_a_non_authenticated_user_should_be_rejected`, `search::search_boards_all_the_matching_private_boards_…` | |
| `permissions_test.go`의 카테고리·공유·검색·채널 표 | — | 플러그인 모드 전용. standalone 응답은 S5의 권한 표 비교가 덮는다 |

추가한 테스트: 사이드바 라우트 13건, 보안 6건(`filing_a_board_into_another_users_category_is_refused`, `filing_a_board_into_an_own_category_works`, `another_users_deleted_category_is_not_handed_out`, `another_users_category_cannot_be_changed`, `reorder_categories_treats_a_hostile_identifier_as_data`, `reorder_category_boards_treats_a_hostile_identifier_as_data`), WebSocket 2건, 공유 7건, 검색 7건.

### 보드, 멤버, boards-and-blocks

| 원본 | Rust (`tests/api/boards.rs`, `boards_and_blocks.rs`) | 비고 |
|---|---|---|
| `integrationtests/board_test.go` `TestGetBoards` (2건) | `get_boards_a_non_authenticated_client_should_be_rejected`, `get_boards_should_only_return_the_boards_that_the_user_is_a_member_of` | |
| `TestCreateBoard` (비로그인, public, private, invalid 3건) | `create_board_a_non_authenticated_user_should_be_rejected`, `create_board_create_public_board`(+`…_and_its_blocks`), `create_board_create_private_board`(+`…_and_its_blocks`), `create_board_create_invalid_board_{invalid_board_type,no_type,no_workspace_id}` | |
| `TestCreateBoardTemplate` (2건) | `create_board_template_create_{public,private}_board_template`(+`…_and_its_blocks`) | |
| `TestGetBoard` (6건) | `get_board_*` 6건 | |
| `TestGetBoardMetadata` 비로그인, 라이선스 없음, 읽기 토큰 | `get_board_metadata_*` 3건 | "query is correct", "Professional license" 케이스는 라이선스가 필요해 해당 없음 |
| `TestPatchBoard` (5건) | `patch_board_*` 5건 | |
| `TestDeleteBoard` (4건), `TestUndeleteBoard` (5건) | `delete_board_*` 4건, `undelete_board_*` 5건 | |
| `TestGetMembersForBoard` (4건) | `get_members_for_board_*` 4건 | |
| `TestAddMember` (7건) | `add_member_*` 7건 | |
| `TestUpdateMember` (5건) | `update_member_*` 5건 | "if the user is a guest"는 플러그인 모드 전용 |
| `TestDeleteMember` (7건) | `delete_member_*` 7건 | |
| `TestDuplicateBoard` (2건) | `duplicate_board_create_and_duplicate_public_board`, `…_from_a_custom_category` | |
| `TestJoinBoard` (5건) | `join_board_*` 5건 | |
| `TestGetAllBlocksForBoard` | `blocks::get_blocks` 등 블록 영역 | |
| `TestGetTemplates` | 템플릿 영역 참고. 템플릿 복제는 `duplicate_board_from_a_template` | |
| `integrationtests/boards_and_blocks_test.go` `TestCreateBoardsAndBlocks` (7건), `TestPatchBoardsAndBlocks` (11건), `TestDeleteBoardsAndBlocks` (5건) | `create_boards_and_blocks_*` 7건, `patch_boards_and_blocks_*` 11건, `delete_boards_and_blocks_*` 5건 | 전부 옮김 |
| `app/boards_test.go` `TestAddMemberToBoard` base case, existing membership | `add_member_to_board_base_case`, `add_member_to_board_return_existing_non_synthetic_membership_if_any` | "synthetic membership 변환"은 standalone에 synthetic 멤버가 없어 해당 없음 |
| `TestPatchBoard` title patch | `app_patch_board_base_case_title_patch` | |
| `TestPatchBoard` 타입 변경 × 사용자 구성(6건, mock 호출 순서) | `patch_board_type_of_a_template_is_broadcast_to_the_workspace_users` | 실제 WebSocket으로 동작을 확인 |
| `TestPatchBoard` 채널 3건 | `app_patch_board_channel` | |
| `TestGetBoardCount` | — | 스토어 전달만. 스토어 테스트가 덮는다 |
| `TestBoardCategory` | `board_category_no_boards_default_category_exists` | |
| `TestDuplicateBoard` (2건) | `duplicate_board_base_case_and_as_template` | |
| `TestGetMembersForBoard`, `TestGetMembersForUser` base·team check false | `get_members_for_board_should_correctly_return_board_members_for_a_valid_board`, `get_members_for_user_returns_the_memberships_as_stored` | "team check true"는 팀 관리자가 없어 해당 없음 |
| `integrationtests/permissions_test.go` (65개 함수) | — | 플러그인 모드 헬퍼로만 돈다. standalone의 라우트 × 역할은 S5의 권한 표 비교가 덮는다 |

추가한 테스트: single-user 토큰 검사(`get_board_in_single_user_mode_needs_the_token`), 오라클로 확인한 본문 검증·권한·와이어 형식 20건, WebSocket 브로드캐스트 4건, 보안 수정 4건.

### 카드, 콘텐츠 블록, 구독

| 원본 | Rust (`tests/api/cards.rs`, `content_blocks.rs`, `subscriptions.rs`) | 비고 |
|---|---|---|
| `integrationtests/cards_test.go` `TestCreateCard` (3건) | `create_card_a_non_authenticated_user_should_be_rejected`, `create_card_good`, `create_card_invalid_card` | |
| `TestGetCards` (3건) | `get_cards_fetch_all_cards`, `get_cards_fetch_with_pagination`, `get_cards_a_non_authenticated_user_should_be_rejected` | |
| `TestPatchCard` (3건) | `patch_card_a_non_authenticated_user_should_be_rejected`, `patch_card_good`, `patch_card_invalid_card_patch` | |
| `TestGetCard` (2건) | `get_card_a_non_authenticated_user_should_be_rejected`, `get_card_good` | |
| `app/cards_test.go` `TestCreateCard`, `TestGetCards`, `TestPatchCard`, `TestGetCard` (9건) | `app_create_card_{success,error}_scenario`, `app_get_cards_{success,error}_scenario`, `app_patch_card_{success,error}_scenario`, `app_get_card_success_scenario`, `app_get_card_not_found`, `app_get_card_error_scenario` | mock 실패 대신 실제 실패(제목 초과, 없는 보드, 잘못된 카드 블록 등) |
| `integrationtests/content_blocks_test.go` `TestMoveContentBlock` (4건) | S6까지 `move_content_block_not_matching_parents`, `…_valid_request_with_not_real_change`, `…_changing_order_with_before`, `…_changing_order_with_after`. S7에서 라우트와 함께 걷어 냈고 `moving_a_content_block_is_no_route`가 404를 본다 | |
| `app/content_blocks_test.go` `TestMoveContentBlock` (9건) | S6까지 `app_move_content_block_*` 8건. S7에서 걷어 냄 | "fail on patchparent"는 mock으로만 만들 수 있는 저장소 실패 |
| `integrationtests/subscriptions_test.go` `TestCreateSubscription` (3건), `TestGetSubscriptions`, `TestDeleteSubscription` (2건) | `create_subscription_create_valid_subscription`, `…_create_invalid_subscription`, `…_create_subscription_for_another_user`, `get_subscriptions_get_subscriptions_for_user`, `delete_subscription_delete_valid_subscription`, `…_delete_invalid_subscription` | |

추가한 테스트: 카드 11건과 페이징 단위 테스트, 콘텐츠 블록 6건(S7에서 라우트와 함께 걷어 냈다), 구독 9건(보안 테스트 4건 포함).

### 파일, 아카이브, 템플릿, 온보딩

이 영역의 코드와 테스트는 하위 에이전트가 작성했고, 작업 도중 모델이 Fable 5.2에서 Opus 4.8로 바뀌었다(루트 `PORTING.md` "작성 모델" 참고).

| 원본 | Rust (`tests/api/files.rs`, `export.rs`, `templates.rs`, `onboarding.rs`) | 비고 |
|---|---|---|
| `integrationtests/file_test.go` `TestUploadFile` (4건) | `files::upload_file_a_non_authenticated_user_should_be_rejected`, `…_without_permissions_is_forbidden`, `…_with_permissions`, `…_reaching_the_max_file_size` | |
| `TestFileInfo` | `files::file_info_of_an_unknown_file_is_null`, `files::file_info_has_the_originals_members` | |
| `integrationtests/user_test.go` `TestTeamUploadFile` (3건) | `files::workspace_upload_file_no_permission`, `…_a_board_admin_can_upload`, `…_a_non_member_is_forbidden` | |
| `app/files_test.go` `TestSaveFile` | `files::upload_maps_jpeg_to_jpg`, `files::upload_answers_a_generated_name`, 단위 테스트 `destination_path_lays_files_out_as_the_original` | 쓰기 실패 케이스는 mock 전용 |
| `TestGetFileInfo`, `TestGetFile`, `TestGetFilePath` | `files::file_info_*`, `files::download_has_the_originals_headers`, `files::download_of_an_unknown_file_is_500` | 저장소·백엔드 오류 주입 케이스는 mock 전용 |
| `TestGetFileReader` (5건) | `files::get_file_reader_moves_a_legacy_root_file` | 오류 주입 4건은 mock 전용 |
| `TestCopyCard`, `TestCopyAndUpdateCardFiles`, `TestCopyCardFiles` | `files::duplicating_a_board_copies_its_files`, `files::duplicating_a_card_copies_its_files` | mock 호출 순서 케이스는 해당 없음 |
| `integrationtests/export_test.go` `TestExportBoard` | `export::export_single_board_round_trips` | |
| `app/import_test.go` `TestApp_ImportArchive` | `export::export_single_board_round_trips`, `export::import_of_*` | 원본 픽스처는 mock 호출 순서를 확인한다. 동작(ID 재생성)은 왕복과 S5 시나리오로 확인. S7에서 형식이 판 3이 되고 원본의 형식(v1 JSONL, v2 ZIP)은 거절한다(`export::archives_of_the_former_formats_are_refused`) |
| `app/templates_test.go` `TestApp_initializeTemplates` (2건) | `templates::template_initialization_is_idempotent_and_measured` | |
| `integrationtests/board_test.go` `TestGetTemplates` | `templates::get_templates_returns_the_built_in_templates` | |
| `integrationtests/work_template_test.go` | `templates::get_templates_has_the_work_template_tracking_ids` | |
| `app/onboarding_test.go` `TestPrepareOnboardingTour` | `onboarding::onboard_duplicates_the_welcome_board_and_records_preferences` | welcome 보드가 없는 오류 경로는 따로 확인하지 않았다 |

추가한 테스트: 다운로드 헤더·Range·조건부 요청, 접근 제어(비로그인, 비멤버, 읽기 토큰, single-user 토큰), 업로드 이름 축약, 내보내기 헤더·권한, 잘못된 아카이브 가져오기(`import_of_a_non_archive`, `…_an_empty_zip`, `…_an_unsupported_version`, `…_a_traversing_entry`), 단위 테스트(날짜, 확장자, Range 해석, 경로 탈출, 콘텐츠 타입, 아카이브 항목 이름).
## 의도적 차이

1. **설정 기본값을 실제로 적용한다.** 원본은 기본값의 키 이름이 설정 키와 달라 `session_expire_time`, `session_refresh_time`, `dbconfig`, `webhook_update` 등에 기본값이 적용되지 않는다(`server/services/config/config.go:75-110`). 포트는 문서화된 기본값(30일, 5시간, `./fvoci.db`)을 쓴다.
2. **환경 변수는 `FVOCI_<키 대문자>`이고 설정 파일에 키가 없어도 적용된다.** 원본은 파일이나 기본값에 있는 키만 환경 변수로 덮을 수 있다. `featureFlags`는 환경 변수로 줄 수 없다. `filess3config`의 자격이 아닌 키는 `FVOCI_FILESS3CONFIG_<키>`다(사용자 결정 2026-11-11 11:05 KST).
3. **지원하지 않는 설정은 시작 오류다:** `authMode`가 `native`가 아님, `prometheusaddress`(→ 2026-11-11부터 지표의 주소), `filesdriver`가 `local`/`amazons3`가 아님, `filess3config.SignV2`, `dbtype: mysql`. `logging_cfg_*`, `audit_cfg_*`는 경고만 낸다. → **정리됨(2026-11-11 13:35 KST):** 이 둘과 `filess3config.trace`는 없앤 키다. 값이 있으면 시작하지 않는다(사용자 결정 11:05 KST).
4. **`/static/` 아래 디렉터리 목록을 보여 주지 않는다.** 원본은 파일 서버가 목록 HTML을 만든다. 포트는 404다.
5. **캐시 헤더와 사전 압축 파일**: 정적 파일은 이름에 내용 해시가 있으면 `public, max-age=31536000, immutable`, 없으면 `no-cache`(ETag 재검증). `index.html`은 `no-cache`. 요청한 파일 옆에 `.br`/`.gz`가 있고 클라이언트가 받으면 그것을 보낸다. 원본에는 없는 동작이다.
6. **파일 저장 경로 검사**: 절대 경로나 `..`이 든 저장 경로는 거절한다. 원본은 호스트 애플리케이션의 파일 저장소에 그대로 넘긴다.
7. **S3 객체 키**: 키 세그먼트에 허용되지 않는 문자는 퍼센트 인코딩된다. 템플릿 보드에 올린 파일(원래 이름을 그대로 쓴다)의 이름에 그런 문자가 있으면 원본이 만든 키와 달라진다. 일반 업로드는 서버가 만든 이름이라 영향이 없다.
8. **API 응답에 `Cache-Control: no-store`를 붙인다.** 원본에는 캐시 헤더가 없다. 핸들러가 자기 정책을 정한 경우(파일 다운로드)는 그대로 둔다.
9. **WebSocket ping.** 서버가 `websocket_ping_seconds`(기본 30초, 0이면 끔)마다 ping 프레임을 보내고, 클라이언트의 `PING`에 `PONG`으로 답하며, 90초 동안 아무것도 보내지 않은 연결을 닫는다. 원본은 셋 다 하지 않는다.
10. **느린 WebSocket 연결을 끊는다.** 연결마다 보낼 메시지를 512개까지 쌓고, 넘치면 그 연결을 닫는다(클라이언트는 다시 연결하며 전체를 새로 읽는다). 원본은 연결마다 뮤텍스를 잡고 동기적으로 쓰므로 느린 연결 하나가 브로드캐스트 전체를 막는다.
11. **CSRF 헤더 검사 범위.** 알려진 API 경로를 다른 메서드로, `X-Requested-With` 없이 부르면 포트는 400을 준다. 원본은 메서드까지 맞아야 미들웨어가 돌기 때문에 이 경우 `index.html`(200)을 준다. 헤더가 있으면 둘 다 `index.html`이다.
12. **웹훅 실패를 견딘다.** 원본은 웹훅 요청이 실패하면 nil 응답을 읽다가 죽는다(`server/services/webhook/webhook.go:33-35`). 포트는 경고를 남기고 다음 주소로 넘어간다. 요청마다 30초 제한을 둔다(원본은 제한 없음).
13. **로그.** `tracing`을 쓴다. `FVOCI_LOG_LEVEL`(기본 `info`, `tracing` 필터 문법), `FVOCI_LOG_FORMAT=json`. 명령행으로 받은 `--dbconfig` 값은 로그에 남기지 않는다(원본은 남긴다. 접속 문자열에 비밀번호가 들어갈 수 있다).
14. **다른 접두사의 환경 변수 경고.** `FVOCI_`가 아닌 접두사로 `…_SINGLE_USER_TOKEN`, `…_HTTP_SERVER_BASEPATH`, `…_SERVERROOT`, `…_DBCONFIG` 등 이 서버의 설정 이름이 붙은 환경 변수가 있으면 "무시됨"이라고 경고한다. 옛 접두사를 별칭으로 받지는 않는다.
15. **텔레메트리 없음.** `/clientConfig`는 원본 웹 앱과의 호환을 위해 `telemetry: false`, `telemetryid: ""`를 고정값으로 내보낸다.
16. **single-user 토큰과 가입 토큰, 읽기 토큰 비교는 상수 시간**이다.
17. **관리자 비밀번호 설정의 빈 비밀번호 거절**(`server/api/admin.go:41`, 사용자 결정). 원본의 검사는 항상 참이라 빈 비밀번호가 200으로 저장된다(오라클 확인, 그 사용자는 로그인할 수 없게 된다). 포트는 `{"password":""}`, `{}`, `null`을 400 `password is required`로 거절한다.
18. **72바이트를 넘는 관리자 비밀번호.** 원본은 해시 단계에서 패닉이 나 응답 없이 연결이 끊긴다(오라클 확인). 포트는 400 `password length exceeds 72 bytes`.
19. **소켓 경로에 다른 파일이 있을 때.** 원본은 무엇이든 지운다(오라클에서 일반 파일이 삭제됨, `server/server/server.go:386-390`). 포트는 소켓 파일만 교체하고 그 밖의 파일이면 시작을 거부한다.
20. **SIGTERM에서도 정상 종료한다.** 원본은 SIGINT만 처리해 SIGTERM에는 소켓 파일을 남기고 죽는다.
21. **TLS 세부.** TLS 1.2/1.3, HTTP/1.1만 지원한다(원본은 HTTP/2도 협상). TLS 포트로 온 평문 요청은 응답 없이 닫는다(원본은 400 안내문).
22. **패닉 처리 범위.** 원본은 `/api/v2`만, 포트는 관리자 소켓을 포함한 모든 라우트에서 표준 500 본문으로 바꾼다.
23. **요청 로그.** 요청마다 메서드, 쿼리 없는 경로, 상태, 소요 시간, 클라이언트 주소를 INFO로 남긴다. `trust_forwarded_for`가 켜져 있고 `X-Forwarded-For`의 첫 항목이 IP 주소로 읽힐 때만 그 값을 클라이언트 주소로 쓴다.
24. **옮기지 않은 시작 작업:** 메트릭 갱신 작업, 텔레메트리 작업과 `system_settings`의 `TelemetryID` 행, `--monitorpid`, 시작 시 버전·설정 전체 로그(원본은 DB 접속 문자열까지 찍는다).
25. **없는 블록의 undelete는 404.** 원본은 nil 역참조로 500(오라클 확인, `server/api/blocks.go:452-458`).
26. **댓글 블록 복제는 404.** 원본은 충돌로 500(오라클 확인, `sqlstore/blocks.go:883-901`).
27. **묶음 패치가 중간에 실패하면 아무것도 적용되지 않는다.** 원본 SQLite는 앞선 패치가 이미 적용된 채 500/400을 준다(스토어의 트랜잭션 차이).
28. **객체 자리에 배열이 온 요청.** `POST [[]]` 등은 원본에서 500, 포트에서는 serde가 위치로 읽어 400 또는 무동작 200이 된다. 실제 클라이언트는 보내지 않는다. 관리자 본문도 같다(`["x"]`가 비밀번호 `x`로 읽힌다).
29. **`"fields": null`은 `{}`로 돌아온다.** 원본은 `null`을 그대로 돌려주고 그런 블록을 패치하면 죽는다.
30. **보드를 카테고리에 넣기 전에 카테고리를 확인한다**(`server/app/category_boards.go:150-170`). 보안 수정의 부수 효과로, 없는·삭제된·다른 팀의 카테고리에 넣으려는 소유자의 요청은 여전히 500이지만 아무것도 쓰이지 않는다. 원본에서는 보드가 그 카테고리로 옮겨져 사이드바에서 사라진다(오라클 확인).
31. **카테고리 브로드캐스트는 응답 전에 인라인으로 보낸다.** 원본은 고루틴이나 알림 큐를 쓴다. 빈 목록으로 받아들여진 reorder의 WebSocket 메시지는 원본에서 `null`, 포트에서 `[]`이다(HTTP 응답은 둘 다 `null`. 코드로 확인).
32. **Go 디코더의 관대함 중 옮기지 않은 것:** 대소문자를 가리지 않는 멤버 이름, reorder 목록 안의 `null` 원소.
33. **`POST /boards-and-blocks`는 모르는 보드 타입과 최소 역할을 400으로 거절한다**(`invalid-board-type`, `invalid-board-minimum-role`). 원본은 이 라우트에서 둘 다 검증하지 않아 타입 `"invalid"`나 역할 `"boss"`가 저장된다(오라클 확인, `server/api/boards_and_blocks.go:84-140`). 모델의 enum이 그런 값을 담지 못한다.
34. **`properties` 없이 만든 보드의 `updatedProperties` 패치가 동작한다.** 원본은 nil 맵 충돌로 500.
35. **빈 목록과 빈 객체.** `PATCH /boards-and-blocks`에 블록이 없으면 원본은 `"blocks": null`, 포트는 `[]`. 보드의 `properties`/`cardProperties`도 `null` 대신 `{}`/`[]`.
36. **JSON 디코드 오류 문구는 serde의 것이다.** 상태 코드는 라우트마다 원본과 같다.
37. **보드 없는 boards-and-blocks 호출의 충돌을 재현하지 않는다**(`server/app/boards_and_blocks.go:26`, `:101`의 `[0]` 인덱싱).
38. **보내지 않는 브로드캐스트:** 채널 연결·해제 안내 메시지(standalone에서는 로그만 남는다), synthetic 채널 멤버용 멤버 브로드캐스트.
39. **브로드캐스트 순서.** 원본은 FIFO 큐 하나, 포트는 작업마다 태스크 하나다. 한 작업 안의 순서는 지켜지지만 요청 사이의 순서는 보장하지 않는다(원본도 워커 10개라 엄밀한 보장은 없다).
40. **카드 목록의 잘못된 `page`/`per_page`는 400 하나만 답한다**(`server/api/cards.go:186-196`). 원본은 `return`이 없어 400 오류 본문 뒤에 카드 목록을 이어 쓴다(둘 다 틀리면 오류 본문 둘과 목록).
41. **`per_page <= 0`이고 `page`가 0이 아닌 카드 목록.** 원본 SQLite는 `LIMIT` 없는 `OFFSET`으로 500(오라클 확인), 포트는 전체 카드. 오프셋이 `i64`를 넘는 페이지는 원본에서 곱셈이 넘쳐 첫 페이지가 나오고, 포트는 `[]`.
42. **삭제된 블록의 구독 해지는 200 `{}`.** 원본은 구독을 지운 뒤 nil 보드를 역참조해 500(오라클 확인, `server/app/subscriptions.go:43-51`).
43. **구독 변경은 브로드캐스트하지 않는다.** 원본 standalone WebSocket 서버의 해당 함수가 비어 있다(`server/ws/server.go:785-787`).
44. **카드 라우트의 객체가 아닌 본문은 원본처럼 400이다**(serde의 위치 기반 해석을 막았다).
45. **카드 단건 조회는 `ViewBoard`로 충분하다**(사용자 결정 2026-11-10). 원본은 `GET /cards/{id}`에 `ManageBoardCards`를 요구해(`server/api/cards.go:363`) viewer와 commenter가 카드 목록은 읽으면서 단건은 403을 받는다(오라클 권한 표로 확인). 보안 결함이 아니라 권한을 잘못 고른 버그이고, 플러그인 저장소도 PR #162에서 같은 수정을 했다. 패치 등 다른 카드 라우트의 권한은 그대로다. 테스트 `cards::get_card_permissions_and_refusals`.
46. **`fields.properties`가 객체가 아닌 카드를 읽을 수 있다**(사용자 결정 2026-11-10). 원본은 그런 카드가 하나 있으면 보드의 카드 목록이 500이고 단건 조회는 400이다(오라클 확인). 포트는 그 카드를 속성 없이(`"properties": {}`) 내보내고 경고 로그를 남긴다. 저장된 블록은 바꾸지 않는다. `contentOrder`·`icon`·`isTemplate`이 잘못된 카드는 원본대로 실패한다. 테스트 `cards::cards_whose_properties_are_no_object_are_read_without_properties`. (S6: 그런 블록은 이제 쓸 수 없다(보안 결정 대기 S14). API로는 만들 수 없게 되어 테스트를 단위 테스트 `app::cards::tests::a_stored_card_whose_properties_are_no_object_is_read_without_properties`로 옮겼다.) → **정리됨(2026-11-11 13:57 KST):** 종류가 맞지 않는 필드는 무엇이든 읽을 때 빼고 경고한다(tracer-bullet 12).
47. **416 응답의 `Content-Length`.** 원본은 Range 오류 전에 정해 둔 파일 크기를 그대로 남겨 본문보다 큰 `Content-Length`를 보낸다. 포트는 실제 본문 길이를 보낸다(상태, `Content-Range: bytes */size`, 본문 문구는 같다).
48. **여러 구간 Range는 전체 파일(200)로 답한다.** 원본은 `multipart/byteranges`(206). 단일 구간은 원본과 같다.
49. **업로드 디렉터리의 날짜(`boards/YYYYMMDD`)는 UTC다.** 원본은 서버 로컬 날짜. 경로는 파일 정보 행에 기록되므로 조회에는 영향이 없다.
50. **확장자만 있는 `fileId`(예: `".png"`)를 가진 블록의 보드 복제는 충돌하지 않는다.** 원본은 슬라이스 범위 오류로 패닉해 500을 답한다(오라클 확인). 포트는 다른 복사 실패와 같게 로그를 남기고 복제를 마친다(200).
51. **파일 라우트의 `HEAD`는 본문 없는 다운로드 헤더를 준다.** 원본은 GET만 등록해 `index.html`이 나온다.
52. **템플릿 초기화는 서버 시작 때 한 번 돈다**(약 0.76초, 보드 13개·블록 372개·파일 14개). 이미 현재 버전의 템플릿이 있는 DB(원본이 만든 DB 포함)에서는 아무것도 하지 않는다.
53. **새로 저장하는 파일의 경로에 보드 ID가 들어간다**: `boards/YYYYMMDD/<보드 ID>/<파일 이름>`. 원본은 `boards/YYYYMMDD/<파일 이름>`. 경로는 파일 정보 행에 기록되고 두 서버 모두 그 값을 읽으므로 서로가 저장한 파일을 찾는다. 이 경로가 파일을 보드에 묶는 근거다(보안 수정 표). 템플릿 보드의 파일은 원본과 같게 `<팀>/<보드>/<이름>`.
54. **내보내기는 같은 파일을 한 번만 쓴다.** 블록 둘이 같은 파일을 가리키면 원본은 같은 이름의 ZIP 항목을 두 번 쓴다. 가져오는 쪽은 이름당 파일 하나만 남기므로 결과는 같다.
55. **보드 타입이 잘못된 아카이브의 가져오기는 500.** 원본은 타입 `"X"`인 보드를 그대로 가져온다(오라클 확인, 200). 모델의 enum이 그런 값을 담지 못한다(차이 33과 같은 이유). 플러그인 저장소는 PR #219에서 이런 보드를 검증으로 거절한다.
56. **내보내기·가져오기·업로드는 내용을 메모리에 둔다.** 원본은 ZIP을 응답으로 흘려 쓰고 항목을 저장소로 흘려 읽는다. 포트는 아카이브 전체와 업로드 파일을 메모리에 올린 뒤 처리한다. 동작은 같지만 큰 첨부가 많은 보드에서는 메모리를 그만큼 쓴다(후속 개선 후보). 아카이브 항목 하나는 256MiB까지만 풀린다.
57. **기본 카테고리는 한 번만 만들어진다.** 원본은 "찾아보고 없으면 만든다"를 아무것도 잡지 않고 해서, 새 사용자의 요청 둘이 함께 오면 기본 카테고리가 둘 생기거나(`server/app/category_boards.go:16-51`), 한쪽이 `board membership not found for user's board`로 실패해 보드가 어느 카테고리에도 들어가지 않는다(포트의 테스트로 관찰. 원본에는 재현하지 않았다). 포트는 생성 구간을 프로세스 안에서 직렬화하고, 멤버십보다 보드를 먼저 읽는다. 테스트 `sidebar::the_default_category_is_created_once_by_concurrent_requests`. 서버 여러 대가 한 DB를 쓰는 경우는 그대로다.
58. **핸들러는 클라이언트가 끊겨도 끝까지 실행된다.** 프레임워크는 연결이 닫히면 핸들러를 버리는데, 그러면 보드는 저장됐지만 카테고리에 안 들어갔거나 브로드캐스트가 빠진 상태가 생긴다. 원본의 핸들러는 항상 끝까지 돈다. API 라우트 전체를 추적되는 태스크로 실행한다(`api::run_to_completion`). 테스트 `system::a_request_is_finished_when_its_client_disconnects`.
59. **종료.** 진행 중인 요청은 10초까지 기다린 뒤 끊는다(원본은 즉시 닫는다). WebSocket 연결은 종료 때 닫는다. 두 번째 인터럽트는 기다리지 않고 프로세스를 끝낸다.
60. **시작 때 거절하는 값.** 빈 single-user 토큰(토큰 없는 요청이 전부 통과하게 된다)과 `/`로 시작하지 않거나 `{`, `}`, `*`가 든 base path는 시작 오류다.
61. **루트 워크스페이스를 읽지 못했을 때 가입 토큰을 새로 쓰지 않는다.** 원본은 읽기 실패를 "팀이 없음"으로 보고 새 토큰을 써서, 일시적인 DB 오류 뒤에 초대 링크가 모두 무효가 된다(`server/app/teams.go`). 포트는 "없음"일 때만 만들고 그 밖의 실패는 500이다.
62. **인증 경로의 스토어 실패를 로그에 남긴다.** 응답(401)은 원본과 같다.
63. **템플릿 가져오기가 도중에 실패하면 남은 내장 템플릿을 지운다.** 보드가 파일보다 먼저 저장되므로, 그대로 두면 다음 시작 때 버전 검사를 통과해 파일 없는 템플릿이 남는다.
64. **`App`의 옛 경로 폴백(`GetFileReader`, `MoveFile`)은 옮기지 않았다.** 원본의 다운로드 핸들러는 첫 오류 검사에서 끝나 그 분기에 닿지 않는다(`server/api/files.go:146-170`). 닿지 않는 코드에 "파일 이름을 그대로 저장 경로로 쓰는 이동"을 두지 않기로 했다.
65. **이름에 `/`가 든 파일 요청.** `…/files/teams/0/{보드}/..%2F..%2Fconfig.json`처럼 디코드하면 `/`가 생기는 이름은 원본에서 라우터가 경로를 정리해 웹 앱(200)으로 끝난다. 포트는 핸들러가 그 이름을 거절해 500을 답한다. 어느 쪽도 파일을 내주지 않는다.

## 보안 수정

원본에서 재현해 확인한 뒤 고친 것만 적는다.

| 원본 위치 | 무엇이 가능했나 | 확인 방법 (오라클, 2026-11-10) | 고친 방식 | 테스트 |
|---|---|---|---|---|
| `server/api/auth.go:331-348` | single-user 모드에서 세션이 선택인 경로(`GET /boards/{id}`, `GET /boards/{id}/blocks`, 파일 다운로드, 파일 정보)는 토큰이 없거나 틀려도 single-user 세션을 받는다. 같은 기기의 다른 프로세스가 토큰 없이 보드를 읽을 수 있다 | single-user 모드로 띄운 뒤 `Authorization` 없이, 그리고 틀린 토큰으로 비공개 보드와 그 블록을 요청 → 200과 내용 | 세션이 선택인 경로에서도 토큰이 맞을 때만 single-user 세션을 준다. 아니면 세션 없음으로 처리해 원본의 비로그인 경로(읽기 토큰 검사 → 401)로 간다 | `users::single_user_mode`(세션 필수 경로), 보드·블록·파일 영역의 single-user 테스트 |
| `server/api/auth.go:247` | 로그인한 사용자가 다른 사용자의 현재 비밀번호를 알면 그 사용자의 비밀번호를 바꿀 수 있다(경로의 `userID`와 세션 사용자를 비교하지 않는다) | 코드 확인. 사용자 결정(2026-11-10)으로 고침 | 세션 사용자와 경로의 사용자가 다르면 403 | `users::change_password_of_another_user_is_forbidden` |
| `server/app/category_boards.go:150-170`, `server/api/categories.go:385` | 다른 사용자의 카테고리에 보드 ID를 넣을 수 있다. 쓰기가 소유자 검사보다 먼저 일어난다 | 사용자 B가 `POST /teams/0/categories/{A의 카테고리}/boards/evil1` → 응답은 500이지만 A의 카테고리에 `evil1`이 나타남 | 쓰기 전에 카테고리가 세션 사용자의 것이고 경로의 팀에 있으며 삭제되지 않았는지 확인. 남의 카테고리면 403 `category doesn't belong to user` | `sidebar::filing_a_board_into_another_users_category_is_refused`, `sidebar::filing_a_board_into_an_own_category_works` |
| `server/app/category.go:104-112` | 이미 삭제된 카테고리를 `DELETE`하면 소유자가 아니어도 그 카테고리 내용을 200으로 받는다 | B가 A의 삭제된 카테고리를 `DELETE` → 200과 A의 카테고리 | 소유자 검사를 "이미 삭제됨" 지름길보다 먼저 한다. 403 | `sidebar::another_users_deleted_category_is_not_handed_out`, `sidebar::delete_category_trying_to_delete_already_deleted_category` |
| `server/services/store/sqlstore/category.go`, `category_boards.go`의 reorder | 카테고리·보드 reorder 목록의 ID가 SQL에 그대로 이어 붙는다 | 카테고리 ID `a' THEN 777 WHEN 'zz`로 원본이 200과 요청과 다른 순서를 돌려줌. 보드 ID `q'q`는 500 `near "q": syntax error` | 스토어가 값을 바인딩한다 | `sidebar::reorder_categories_treats_a_hostile_identifier_as_data`, `sidebar::reorder_category_boards_treats_a_hostile_identifier_as_data` |
| `server/api/members.go:164-168`, `server/app/boards.go:508-554` | 공개 보드의 editor가 `schemeAdmin: true`로 멤버를 추가할 수 있다. 공개 보드는 누구나 editor로 참여할 수 있으므로, 계정 두 개만 있으면 어떤 공개 보드든 빼앗을 수 있다 | user2가 참여 → `POST …/members {userId: user3, schemeAdmin: true}` → 200. user3이 user2를 승격시키고 소유자를 제거한 뒤 보드를 비공개로 바꿈 → 소유자의 `GET /boards/{id}`가 403 | `schemeAdmin` 부여에는 `ManageBoardRoles`가 필요. 아니면 그 라우트의 기존 거절인 403 `access denied to modify board members`. 원본 웹 앱은 역할 관리 권한이 없는 사용자로 admin 플래그를 보내지 않는다 | `boards::add_member_as_admin_needs_the_permission_to_manage_roles`, `boards::add_member_by_an_editor_of_an_open_board_and_as_admin_by_an_admin_still_work` |
| `server/api/boards_and_blocks.go:285-296` (`server/api/boards.go:392-397`와 비교) | 단일 보드 라우트에서 403인 `channelId` 변경을 editor가 `PATCH /boards-and-blocks`로는 할 수 있다 | editor의 `PATCH /boards/{id} {channelId}` → 403, 같은 내용의 `PATCH /boards-and-blocks` → 200 | 묶음 라우트에도 같은 `ManageBoardRoles` 검사와 문구 | `boards_and_blocks::patch_boards_and_blocks_channel_needs_the_permission_to_manage_roles`, `…_by_an_editor_and_channel_by_an_admin_still_work` |
| `server/api/subscriptions.go:84-96` | 로그인한 사용자가 볼 수 없는 보드의 블록에도 구독을 만들 수 있다 | 비멤버가 비공개 보드의 카드를 구독 → 200, 자기 구독 목록에 나타남. standalone에서는 알림이 로그로만 가므로 지금 새는 내용은 없지만 남의 보드에 상태가 쌓인다 | 블록 조회 뒤 그 보드의 `ViewBoard`를 요구. 아니면 이 파일의 기존 거절인 403 `access denied`. 원본 웹 앱은 이 라우트를 부르지 않는다 | `subscriptions::subscription_to_a_block_of_a_board_the_user_cannot_view_is_refused`, `subscriptions::subscription_by_every_member_of_the_board_still_works` |
| `server/api/content_blocks.go:67-86`, `server/app/content_blocks.go:11-14` | 콘텐츠 블록 이동의 대상 블록이 권한 없는 보드에 있어도, 오류 문구로 그 블록의 부모 카드 ID를 알려 준다 | 자기 블록을 남의 비공개 보드 블록 "뒤로" 이동 → 400 `not matching parent <자기 카드> and <남의 카드 ID>` | 대상이 다른 보드에 있으면 그 보드의 `ManageBoardCards`도 요구. 아니면 403 `access denied to modify board cards` | `content_blocks::move_content_block_next_to_a_block_of_a_board_without_permission_is_refused`, `content_blocks::move_content_block_within_a_card_and_between_own_boards_is_as_before`(S7에서 라우트와 함께 걷어 냈다) |
| `server/api/blocks.go:297-303`, `server/app/files.go` `CopyCardFiles` | 블록 삽입의 `sourceBoardID`에 권한 검사가 없어, 볼 수 없는 보드를 지정해 그 보드의 파일을 서버가 복사하게 할 수 있다(파일 이름은 알아야 한다). 플러그인 저장소는 PR #268에서 같은 결함을 고쳤다 | bob이 alice의 비공개 보드를 `sourceBoardID`로 주고 alice의 파일 이름을 단 이미지 블록을 자기 보드에 삽입 → 저장소의 파일 수가 15 → 16 → 17로 늘어남(응답은 404, 첨부 블록은 200) | 복사 전에 그 보드의 블록을 읽을 수 있는 사용자인지 확인(멤버, 또는 공개 템플릿의 팀 사용자). 아니면 403 `access denied to source board`(플러그인 수정과 같은 문구). 원본 웹 앱에는 이 매개변수를 보내는 호출이 없다 | `blocks::post_blocks_with_a_source_board_the_user_cannot_read_is_refused`, `blocks::post_blocks_with_a_source_board_the_user_can_read_still_works` |
| `server/api/blocks.go:372` (`handleDeleteBlock`) | 댓글 삭제에 작성자 확인이 없어 editor가 남의 댓글을 지울 수 있다. 플러그인 저장소는 PR #135에서 고쳤다 | 오라클 권한 표: 소유자가 쓴 댓글을 editor가 `DELETE` → 200 | 사용자 결정(2026-11-10)으로 플러그인과 같게 고침. 댓글 블록은 작성자 본인이거나 `ManageOthersComments`(admin)를 가진 사용자만 삭제, 아니면 403 `access denied to delete other users' comments`. 원본 웹 앱은 자기 댓글이거나 admin일 때만 삭제 메뉴를 보여 준다 | `comments::delete_comment_of_another_user_needs_the_permission_to_delete_others_comments`, `comments::delete_comment_by_its_author_or_by_an_admin_works`, `comments::a_commenter_deletes_and_brings_back_their_own_comment`, `comments::the_author_of_a_comment_brings_back_only_what_they_deleted` |
| `server/api/files.go`, `server/app/files.go` `SaveFile` | (예방) 업로드 파일 이름은 클라이언트가 정한다. 원본은 Go의 multipart 처리가 이름을 기본 이름으로 줄여 준다(오라클 확인: `../../evil.sh` → `evil.sh`) | 오라클에서 같은 축약을 확인. 포트의 multipart 처리는 이름을 줄여 주지 않으므로 직접 줄인다 | 저장 전에 기본 이름으로 축약. 저장 계층도 `..`과 절대 경로를 거절 | `files::upload_name_with_path_separators_is_reduced_to_its_base` |
| `server/app/import.go` | 아카이브 항목 이름의 `..`과 절대 경로, 압축 폭탄. 원본은 `board.jsonl`만 70MB로 제한하고 나머지 항목은 제한이 없다 | 코드 확인(원본에 대한 폭탄 재현은 하지 않았다) | 항목 이름을 정리한 디렉터리와 기본 파일 이름으로 나눠 업로드 루트 밖에 쓰지 않는다. 항목당 256MiB까지만 읽는다 | `export::import_of_a_traversing_entry_is_harmless` |
| `server/api/files.go:146-170`, `server/app/files.go` `GetFilePath`·`CopyCardFiles`, `server/app/export.go` | 파일이 보드에 묶여 있지 않다. 아무 보드나 읽을 수 있는 사용자는 이름을 아는 파일을 그 보드의 경로로 받는다. 서버가 이름으로 파일을 읽는 다른 경로(보드·카드 복제의 파일 복사, 내보내기)로도 남의 파일 내용을 얻을 수 있다. 플러그인 저장소는 PR #114·#179·#247·#268에서 고쳤다 | bob이 alice의 비공개 보드에 올린 파일을 alice의 보드 경로로 요청 → 403, 자기 보드 경로로 요청 → 200과 파일 내용 | 포트가 저장하는 파일은 경로에 보드 ID를 넣고(의도적 차이 53), 그 보드가 아닌 보드를 통한 다운로드는 403 `file does not belong to the specified board`, 정보 조회는 403 `access denied to file`(플러그인 수정과 같은 문구). 복제는 그런 파일을 복사하지 않고, 내보내기는 싣지 않는다. 원본 배치(경로에 보드가 없는)의 파일은 원본대로 준다(결정 대기 S16. **S6에서 정리됨:** 이제 예외가 없다. 경로가 보드를 말하지 않는 파일은 주지 않는다). 원본 웹 앱은 블록의 보드 경로로만 파일을 요청한다 | `files::a_file_is_not_given_out_through_another_board`, `files::a_file_is_given_out_through_its_own_board`, `files::a_file_of_another_board_is_neither_copied_nor_exported`, `files::a_file_in_the_originals_layout_is_served_as_the_original_does`(S6에서 `files::a_file_whose_path_names_no_board_is_given_out_through_no_board`로 바뀜) |

## 보안 결정 대기

고치면 정상 클라이언트의 동작이나 와이어 형식이 바뀌는 항목이다. 답을 받을 때까지 원본 동작을 유지한다. 루트 `PORTING.md`의 결정 항목 표에 같은 내용이 있다.

| # | 원본 위치 | 내용 | 현재 동작 |
|---|---|---|---|
| S1 | `server/api/teams.go:106-131` | 로그인한 사용자는 누구나 `GET /teams/{아무 ID}`로 가입 토큰을 읽는다. 원본 웹 앱의 "초대 링크 복사"가 이 값을 쓰므로 응답에서 빼면 그 기능이 달라진다 | 원본대로 |
| S2 | `server/api/auth.go:25` | 로그인한 사용자는 누구나 가입 토큰을 재발급할 수 있다(관리자 개념이 없다) | 원본대로 |
| S3 | `server/api/admin.go` | 관리자가 비밀번호를 재설정해도 그 사용자의 기존 세션이 살아 있다. 끊으면 다른 기기가 로그아웃된다 | **S6에서 정리됨(2026-11-10).** 재설정하면 그 이름의 사용자가 가진 세션을 모두 지우고(비밀번호 변경과 한 트랜잭션), 그 사용자로 인증한 WebSocket 연결을 닫는다. 재설정이 거절되면(빈 비밀번호, 없는 사용자) 세션은 그대로다. 다른 사용자의 세션은 건드리지 않는다. `Store::reset_user_password`, `App::reset_user_password`, `Hub::close_user`. 테스트 `admin::socket::admin_set_password_ends_every_session_of_the_user`, `…_leaves_the_sessions_of_other_users`, `a_refused_admin_set_password_keeps_the_sessions`, `websocket::resetting_a_password_closes_the_websocket_connections_of_that_user`, 스토어 `sessions::reset_user_password_deletes_the_sessions_of_those_users_only` |
| S4 | `server/web/webserver.go:113` | `usessl`인데 인증서 파일이 없으면 조용히 평문 HTTP로 뜬다. 시작을 거부하는 편이 안전하지만 운영자가 보는 동작이 바뀐다 | **S6에서 정리됨(2026-11-10).** `usessl`인데 인증서나 키 파일이 없으면 연결을 하나도 받지 않고 오류로 끝난다. 오류는 없는 파일을 이름으로 알려 주고 "`usessl`을 끄면 평문으로 뜬다"고 안내한다(바이너리로 확인: 종료 코드 1). `usessl`이 꺼져 있으면 파일을 보지 않는다. 테스트 `admin::use_ssl_without_certificate_files_refuses_to_serve`, `admin::use_ssl_with_a_certificate_but_no_key_refuses_to_serve`, `admin::without_use_ssl_missing_certificate_files_do_not_matter`, 허용되는 쪽은 기존 `admin::use_ssl_serves_over_tls` |
| S5 | `server/api/blocks.go` 전반 | 요청 본문 크기에 제한이 없다. 로그인한 사용자가 서버 메모리를 원하는 만큼 쓰게 할 수 있다. 블록 하나의 `fields`가 정당하게 2MB를 넘을 수 있어 일괄 제한은 정상 요청을 막을 수 있다 | **S6에서 정리됨(2026-11-10 18:37 KST), JSON 본문 부분.** JSON을 받는 라우트는 `max_request_bytes`(기본 4 MiB), 세션 없이 부르는 로그인·가입은 64 KiB, WebSocket 메시지는 64 KiB까지만 읽는다. 넘으면 413 `payload_too_large`와 `limit`(WebSocket은 답할 길이 없어 연결을 끊는다). 본문은 한도까지만 메모리에 두고, 거절한 본문은 8 MiB·5초까지 읽어 버린 뒤 답한다(쓰는 중에 연결이 닫히면 클라이언트가 413을 못 읽는다. 버리기 전에는 4 MiB+1바이트 요청 9번 중 1번이 broken pipe였다). 블록 하나의 최대 크기(제목 16,383자 + 필드 800,000자, UTF-8로 약 3.3 MB)는 4 MiB에 들어간다. 테스트 `tests/api/limits.rs`, 단위 `api::limits`. **업로드(2026-11-11 09:53 KST):** 파일은 청크 업로드(설계 5.6)로 100 MiB(`maxfilesize`)까지 받고, 요청 하나짜리 `uploadFile`은 흘려 받으며 부분 하나(8 MiB)까지다. **남은 것:** 가져오기·내보내기의 한도와 스트리밍 |
| S6 | `server/api/blocks.go` undelete·duplicate·묶음 패치 | 비멤버가 403과 404의 차이로 보드·블록의 존재 여부를 알 수 있다. 고치면 정상 클라이언트가 보는 상태 코드가 바뀐다 | **S6에서 정리됨(2026-11-10 18:37 KST).** 규칙 B(설계 1.4): 볼 수 없는 것은 없는 것과 같은 404, 볼 수는 있는데 못 하는 것은 403. 비공개 보드의 비멤버는 그 보드와 블록·카드·멤버·공유·파일의 모든 라우트에서 없는 보드와 같은 답을 받는다. 테스트 `visibility::what_a_user_cannot_see_answers_as_what_does_not_exist`가 42개 요청을 "있지만 숨겨진 것"과 "없는 것"으로 한 번씩 보내 상태·코드·문구가 같은지 비교한다 |
| S7 | — | (보안 수정 표로 옮김: 삽입의 `sourceBoardID` 권한 검사) | — |
| S8 | `server/api/sharing.go` | 공유 토큰을 클라이언트가 정한다. 한 글자 토큰, 여러 보드에 같은 토큰, 빈 토큰(`enabled: true`)도 저장된다(빈 토큰은 조회되지 않아 쓸 수 없다). 서버가 토큰을 만들면 와이어 계약이 바뀐다 | 원본대로 (`sharing::post_sharing_stores_the_token_the_client_chose`) |
| S9 | `server/app/category.go` `UpdateCategory` | 남의 카테고리에 대한 `PUT`이 상태에 따라 404·403으로 갈려 ID의 존재 여부를 알 수 있다. ID는 무작위다 | **S6에서 정리됨(2026-11-10 18:37 KST).** 남의 카테고리, 다른 워크스페이스의 카테고리, 지워진 카테고리, 없는 카테고리가 수정·삭제·보드 넣기·숨기기·보드 재정렬에서 모두 404 `category not found`다(`App::user_category`). 지워진 카테고리를 다시 지우면 전에는 그 카테고리를 답했지만 이제 404다 |
| S10 | — | (의도적 차이 57로 옮김: 기본 카테고리 생성의 경쟁 조건은 포트에서 관찰되어 고쳤다) | — |
| S11 | `server/api/cards.go:279-284`, `content_blocks.go`, `subscriptions.go` | 카드 조회·패치, 이동, 구독 생성이 권한보다 존재 확인을 먼저 해, 로그인한 사용자가 블록 ID의 존재 여부(카드 라우트는 카드인지까지)를 알 수 있다. S6과 같은 종류 | **S6에서 정리됨(2026-11-10 18:37 KST).** ID만 받는 라우트는 블록의 보드를 읽을 수 없으면 없는 블록과 같은 404를 답한다(`require_permission_through`). 카드 라우트는 카드가 아닌 블록도 같은 404다. 보드를 읽을 수 있지만 권한이 모자라면 403 |
| S12 | `server/app/subscriptions.go` | 보드에서 빠진 멤버의 구독이 남는다(목록 조회와 해지는 가능, 새 구독은 불가). `subscriberType: "channel"`에 사용자 ID를 넣어도 저장된다 | **S6에서 정리됨(2026-11-10), 앞부분.** 멤버가 보드에서 빠지면(관리자가 제거하든 스스로 나가든) 그 사용자가 그 보드의 블록에 건 구독을 멤버십 삭제와 같은 트랜잭션에서 끝낸다(해지와 같은 표시. 지워진 블록의 구독도 포함). 다른 보드의 구독과 남아 있는 멤버의 구독은 그대로이고, 제거가 거절되면 아무것도 바뀌지 않는다. 다시 멤버가 되면 다시 구독할 수 있다. 테스트 `subscriptions::leaving_or_being_removed_from_a_board_ends_the_subscriptions_to_its_blocks`, `subscriptions::subscription_by_every_member_of_the_board_still_works`(빠진 뒤에는 구독이 없고 해지는 404), 스토어 `subscriptions::delete_member_ends_the_subscriptions_of_that_user_to_the_blocks_of_that_board`. **남은 것:** `subscriberType: "channel"`은 그대로 저장된다. 채널 개념을 없앨 때 함께 정리한다(`docs/s6-design.md` 1.6). **S6에서 정리됨(2026-11-10 18:37 KST), 남은 부분:** `subscriberType`은 `user`만 받는다. `channel`은 400이다(결정 4). |
| S13 | — | (보안 수정 표로 옮김: 댓글 삭제의 작성자 확인. 사용자 결정 2026-11-10) | — |
| S14 | `server/model/{block,board,card}.go`의 검증 | 카드 속성 정의와 값의 모양을 검증하지 않는다. `cardProperties`에 `id` 없는 정의나 문자열이 아닌 `name`·`type`·`options`, 카드 속성 값에 객체나 숫자, 블록의 `fields.properties`에 문자열이나 목록이 그대로 저장된다(오라클과 포트 모두 200). `fields.properties`가 객체가 아닌 카드가 하나 있으면 그 보드의 카드 목록 API가 500이 되고, 웹 앱은 문자열과 문자열 배열만 다루므로 다른 멤버의 화면이 깨질 수 있다. 플러그인 저장소는 PR #243(과 #216의 모델 부분)에서 이런 입력을 400으로 거절한다. 원본이 받던 입력을 거절하게 되고, 기존 DB·아카이브에 그런 값이 있으면 가져오기가 달라진다 | **S6에서 정리됨(2026-11-10).** 쓰기 검증을 넣었다. 잘못된 모양은 400이고 아무것도 저장되지 않는다. 규칙(플러그인 PR #243과 같은 규칙을 새로 구현): 속성 **값**은 문자열, 문자열 목록, `null` 중 하나. 블록의 `fields.properties`는 있으면 그런 값들의 객체(블록 종류와 무관). 속성 **정의**는 비어 있지 않은 문자열 `id`, 있으면 문자열인 `name`·`type`, 있으면 `id`·`value`·`color`가 문자열인 객체들의 목록인 `options`. 그 밖의 멤버는 그대로 둔다. 검증은 모델(`fvoci_model::validate_card_property_*`, `Block::validate`, `Board::validate`, `BoardPatch::validate`)에 있고, 스토어가 보드와 블록을 쓰는 한 곳씩(`insert_board`, `patch_board`, `insert_block`)에서 부르므로 모든 경로에 걸린다: 보드 생성·패치, 블록 삽입·패치·묶음 패치, 카드 생성·패치, boards-and-blocks 생성·패치, 복제, 아카이브 가져오기. 고치기 전 동작(정의 8가지와 값 5가지가 모두 200)을 먼저 테스트로 확인했다. 내장 템플릿은 검증을 통과한다. 읽기 쪽의 견딤(의도적 차이 46)은 DB에 직접 쓴 값에 대비해 남겼다. 테스트 `properties::*` 9건(경로마다 거절되는 경우와 받아들여지는 경우), 모델 단위 테스트 6건(`card_property::tests`), 스토어 `boards::boards_with_malformed_card_property_definitions_are_not_stored`·`blocks::blocks_with_malformed_properties_are_not_stored`, 서버 단위 테스트 `app::cards::tests::a_stored_card_whose_properties_are_no_object_is_read_without_properties` |
| S15 | `server/api/members.go:164-190` | 역할 관리 권한이 없는 추가자(공개 보드의 editor)가 새 멤버의 역할을 정한다. admin 부여는 포트가 막았지만(보안 수정 표), editor·commenter·viewer는 요청대로 들어가고 플래그가 하나도 없으면 역할 없는 멤버가 된다. 플러그인 저장소는 PR #207에서 이 경우 요청의 역할을 무시하고 보드의 최소 역할을 준다. 원본 웹 앱의 공유 대화상자가 보내는 역할 플래그의 결과가 달라진다 | **S6에서 정리됨(2026-11-10).** 역할 관리 권한(`ManageBoardRoles`)이 없는 추가자(공개 보드의 editor)가 추가한 멤버는 요청의 역할 플래그와 무관하게 **보드의 최소 역할**을 받는다. 최소 역할이 빈 값이면 **viewer**다(사용자 결정. 플러그인 PR #207은 이 경우 editor를 준다). 응답과 저장된 멤버십이 실제로 준 역할을 담는다. admin 플래그를 요청하면 전처럼 403이다. 역할 관리 권한이 있는 추가자는 전처럼 요청한 역할 그대로다(역할 없는 요청은 역할 없는 멤버. 보존한 quirk). 테스트 `boards::add_member_without_managing_roles_gives_the_minimum_role_of_the_board`(최소 역할 4가지 × 요청 2가지, 받은 역할로 실제 권한까지 확인), `boards::add_member_by_a_manager_of_roles_gives_the_roles_of_the_request`, `boards::add_member_as_admin_needs_the_permission_to_manage_roles`. 아래 "S15 비교"의 "플러그인 수정 뒤" 열에서 빈 값의 결과만 viewer로 다르다 |
| S16 | `server/app/files.go`, `server/app/blocks.go` | 파일 소유 검증의 나머지. (1) 원본 배치로 저장된 파일(`boards/YYYYMMDD/<이름>`, 원본이 만든 DB의 모든 파일)과 파일 정보 행이 없는 옛 파일은 경로에 보드가 없어 여전히 아무 보드로나 받을 수 있다. (2) 템플릿 배치(`<팀>/<보드>/<이름>`)의 파일도 다른 보드 경로로 받을 수 있다. 플러그인 저장소는 이 경우 "그 보드의 이미지·첨부 블록이 이 파일을 가리키는가"로 판단하고(PR #114), 그 판단이 뚫리지 않게 블록 삽입·패치 때 파일 소유를 검증해 남의 파일을 가리키는 블록을 거절한다(PR #179, #247). 쓰기 검증은 원본이 받던 블록 쓰기를 거절하게 된다(예: 원본 배치 파일을 가리키던 유일한 블록을 지운 뒤 되살리는 실행 취소) | **S6에서 정리됨(2026-11-10).** 파일의 소유를 저장 경로에서 읽는 규칙을 모든 배치에 적용한다: 업로드 배치(`boards/<날짜>/<보드>/<이름>`)와 템플릿 배치(`<팀>/<보드>/<이름>`, 기록이 없는 파일의 경로이기도 하다) 모두 경로가 보드를 말하고, 그 보드를 통해서만 다운로드·정보 조회·복사·내보내기가 된다. 경로가 보드를 말하지 않는 파일(원본 배치 `boards/<날짜>/<이름>`)은 어느 보드로도 나가지 않는다. 고치기 전 동작을 먼저 테스트로 확인했다: 보드를 템플릿으로 복제해 생긴 파일을 그 템플릿의 멤버가 아닌 사용자가 자기 보드 경로로 받았다(200). 블록 쓰기 검증(PR #179, #247)은 넣지 않았다. 남의 파일을 가리키는 블록은 저장되지만 그 파일이 어느 경로로도 나오지 않으므로 필요가 없고, 실행 취소 같은 정상 쓰기를 거절하지 않는다. `app::files::belongs_to_board`. 테스트 `files::a_template_file_is_not_given_out_through_another_board`, `files::a_template_file_is_given_out_through_its_template`, `files::a_file_whose_path_names_no_board_is_given_out_through_no_board`, 기존 `files::a_file_is_not_given_out_through_another_board`·`a_file_is_given_out_through_its_own_board`·`a_file_of_another_board_is_neither_copied_nor_exported`, 단위 테스트 2건 |
| S17 | `server/api/archive.go`, `server/app/import.go` | 가져오기의 크기 제한. 포트는 항목당 256MiB까지만 푼다(원본은 `board.jsonl`만 70MB로 제한). 플러그인 저장소는 PR #220에서 항목 한도를 `min(70MB, maxfilesize)`로 하고, 요청 본문을 `maxfilesize`로 제한해 413을 답하며, 오류를 200 본문 대신 표준 오류 응답으로 바꿨다. 뒤의 둘은 정상 요청의 응답이 달라진다 | 항목당 256MiB 제한만. **S6에서 정리됨(2026-11-10 18:37 KST), 오류 응답 부분:** 가져오기의 오류는 200의 본문이 아니라 표준 오류 응답(400)이다. **남은 것:** 요청 본문과 항목의 한도는 설계 5.6의 확인 뒤 |
| S18 | `server/app/onboarding.go:47-66` | 온보딩이 welcome 보드를 팀 `0`의 공개 템플릿 중 제목(`Welcome to Boards!`)만으로 찾는다. standalone의 루트 팀도 `0`이라 사용자가 만든 같은 제목의 공개 템플릿이 후보에 섞이고, 쿼리에 정렬이 없어 어느 쪽이 뽑힐지는 행 순서에 달렸다. 오라클에서 같은 제목의 사용자 템플릿을 만든 뒤 새 사용자를 온보딩했을 때는 내장 보드가 뽑혔다(추정, 재현 안 됨). 고친다면 "시스템이 만든 보드" 조건을 더하면 된다 | **S6에서 정리됨(2026-11-10).** 온보딩이 복사할 보드를 제목이 아니라 표식으로 찾는다: 전역 팀의 템플릿 가운데 **시스템이 만들었고**(`createdBy`가 시스템 사용자) 표식 속성 `onboarding: true`를 가진 보드. 속성은 내장 아카이브를 가져올 때 welcome 보드에 붙인다. 보드의 `createdBy`는 어느 라우트에서나 세션의 사용자로 정해지므로 클라이언트가 흉내 낼 수 없다(속성과 제목은 흉내 낼 수 있다). 사용자가 받는 복사본에서는 표식을 뺀다. 표식이 생기기 전에 가져온 내장 템플릿만 있는 DB는 다음 시작 때 템플릿을 다시 가져온다. 고치기 전 동작을 먼저 테스트로 재현했다: 내장 템플릿이 없는 서버에서, 한 사용자가 만든 같은 제목의 공개 템플릿이 다른 사용자의 온보딩 보드로 복사됐다. 테스트 `onboarding::onboarding_without_the_built_in_templates_does_not_copy_a_user_template`, `onboarding::onboarding_copies_the_built_in_board_whatever_users_made`(같은 제목·표식 속성·`createdBy` 요청을 단 사용자 템플릿 5개가 있어도 내장 보드를 복사), `onboarding::onboarding_finds_the_built_in_board_by_its_mark_not_by_its_title`, `templates::templates_without_the_onboarding_mark_are_imported_again`, 단위 테스트 3건(`app::templates::tests`) |

### S15 비교: 역할 관리 권한이 없는 사용자가 멤버를 추가할 때

원본 웹 앱의 공유 대화상자가 보내는 값(그대로 저장된다)과 플러그인 PR #207의 수정이 줄 값을 보드의 최소 역할별로 비교했다. 오라클에서 공개 보드의 editor가 보낸 역할 플래그가 그대로 저장되는 것을 확인했다.

| 보드의 `minimumRole` | 원본 웹 앱이 보내는 플래그 (현재 결과) | 플러그인 수정 뒤 | 같은가 |
|---|---|---|---|
| 빈 값 | viewer | **editor** | 다르다(권한이 달라진다) |
| `viewer` | viewer | viewer | 같다 |
| `commenter` | commenter + viewer | commenter | 플래그만 다르다(권한은 같다) |
| `editor` | editor + commenter + viewer | editor | 플래그만 다르다(권한은 같다) |

멘션과 사람 속성의 "보드에 추가" 확인 창(`markdownEditorInput.tsx:128-139`, `properties/person/confirmPerson.tsx:73-86`)은 사용자가 고른 역할의 플래그를 보낸다. 플러그인 수정 뒤에는 역할 관리 권한이 없는 추가자가 고른 역할이 무시된다.

## 보존한 quirk

S6의 1단계(오류 응답과 경로 규칙)에서 이 규칙에 걸린 항목을 한꺼번에 정리했다. 정리한 항목에는 "S6에서 정리됨"과 지금의 동작을 붙였고, 표시가 없는 항목은 그대로다.

- **세션은 요청마다 갱신된다.** 저장소가 세션의 `update_at`을 읽지 않아 항상 0이고, 그래서 "갱신 시점이 지났나" 검사가 늘 참이다(`server/auth/auth.go:46-53`, `sqlstore/session.go`).
- **`GET /teams/{아무 ID}`는 루트 팀과 `signupToken`을 돌려준다**(`server/api/teams.go:106-131`). → **정리됨:** `GET /w/{workspaceId}`는 그 워크스페이스에서 일하는 사용자에게만 답하고, `signupToken`은 없다(초대 링크가 대신한다).
- **이메일 로그인의 not-found 검사가 반전되어 있다**(`server/app/auth.go:89-96`). not-found가 아닌 DB 오류가 삼켜질 뿐, 클라이언트가 보는 응답은 같다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 로그인 실패(없는 사용자, 틀린 비밀번호)는 401 `unauthorized`·`reason: invalid_credentials` 하나이고, 저장소 오류는 삼키지 않고 500이다.
- **로그인·가입·비밀번호 변경 등의 잘못된 JSON 본문은 500이다.** 디코드 오류를 분류하지 않고 넘긴다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 잘못된 본문은 어느 라우트에서나 400 `bad_request`다(설계 1.3).
- **없는 경로와 메서드가 틀린 경로는 200과 `index.html`이다.** → **S6에서 정리됨(2026-11-10 18:37 KST):** 경로 규칙(설계 2.3)을 구현했다. API 접두어 아래의 모르는 경로와 끝 슬래시는 JSON 404, 아는 경로의 다른 메서드는 JSON 405와 `Allow`, 화면 경로의 `GET`·`HEAD`만 `index.html`이고 끝 슬래시는 308, 그 밖의 메서드는 405(`Allow: GET, HEAD`). 테스트 `tests/api/paths.rs`(기준 경로 없이, 있을 때, 끝 슬래시로 설정했을 때).
- **single-user 모드에서 `POST /users ["single-user"]`와 `GET /users/single-user`는 404다.** 가상 사용자를 만든 뒤 "누가 누구를 볼 수 있나"를 정하려고 DB에서 다시 찾는다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 둘 다 가상 사용자를 답한다(`users::single_user_mode`).
- **카테고리 WebSocket 이벤트는 사용자의 연결 중 팀을 구독한 첫 번째 하나에만 간다**(`server/ws/server.go` `getListenerForUser`). 다른 탭은 받지 못한다. → **정리됨(2026-11-11 13:10 KST):** 그 워크스페이스를 구독한 연결 모두에 간다.
- **블록 구독과 보드 멤버 구독을 함께 가진 연결은 같은 블록 변경을 구독 수만큼 받는다.**
- **`/users/me?channelID=x`는 채널이 있든 없든 `permissions: ["create_post"]`를 붙인다**(standalone의 채널 권한 검사는 항상 참). → **S6에서 정리됨(2026-11-10 18:37 KST):** `channelID`를 읽지 않는다. `permissions`는 응답에 없다.
- **세션 정리 기준은 `max(31일, session_expire_time)`이다**(`server/server/server.go:258-261`). 10분마다 돈다.
- **`usessl`인데 인증서 파일이 없으면 평문 HTTP로 뜬다**(`server/web/webserver.go:113`, 오라클 확인). 포트는 경고를 한 줄 남긴다. 보안 결정 대기 항목이기도 하다. → **S6에서 정리됨:** 이제 시작을 거부한다(보안 결정 대기 S4).
- **컴플라이언스 라우트의 거절은 403이 아니라 401이다**(`server/api/compliance.go:75-78` 등). 히스토리 두 라우트는 권한보다 `modified_since`를 먼저 본다(없으면 400). → **S6에서 정리됨(2026-11-10 18:37 KST):** 라우트를 없앴다(결정 4). 그 경로는 다른 모르는 경로처럼 404다(`admin::routes_of_the_host_application_do_not_exist`).
- **채널·통계 라우트는 501 `not permitted in standalone mode`.** → **S6에서 정리됨(2026-11-10 18:37 KST):** 라우트를 없앴다(결정 4). 404.
- **관리자 라우트의 없는 사용자와 잘못된 본문은 둘 다 500이다.** TCP로 관리자 경로를 부르면 다른 모르는 경로처럼 `index.html`(200)이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 없는 사용자는 404 `user not found`, 잘못된 본문은 400이다. TCP로 부른 관리자 경로는 JSON 404, 소켓의 모르는 경로와 다른 메서드도 JSON 404·405다.
- **관리자가 비밀번호를 바꿔도 기존 세션은 유지된다**(오라클 확인). → **S6에서 정리됨:** 이제 그 사용자의 세션을 모두 끊는다(보안 결정 대기 S3).
- **`securecookie` 설정은 효과가 없다.** 서버는 쿠키를 읽기만 하고 설정하지 않는다.
- **블록 삽입은 전부 아니면 전무가 아니다**(`server/app/blocks.go:182-191`). 두 번째 블록에서 400이 나도 첫 블록은 저장되어 있다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 한 요청의 블록은 한 트랜잭션으로 들어간다. 하나가 거절되면 아무것도 저장되지 않는다(`blocks::post_blocks_size_limits`). 여러 보드의 블록이 섞인 요청은 400이다.
- **빈 목록 삽입(`[]`, `null`)은 보드가 있든 없든, 권한 검사 없이 200 `[]`이다**(`server/api/blocks.go:237-283`). → **S6에서 정리됨(2026-11-10 18:37 KST):** 보드를 볼 수 있어야 한다(볼 수 없으면 404). `[]`는 200 `[]`, `null`은 400이다.
- **삽입은 권한보다 블록 검증을 먼저 한다.** 비멤버도 400(`missing type…` 등)을 본다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 보드를 볼 수 없는 사용자는 본문과 무관하게 404를 받는다. 볼 수 있는 사용자는 본문 검증(400) 뒤에 내용에 따른 권한(403)을 받는다. 어떤 권한이 필요한지가 본문(댓글인지 아닌지)에 달려 있어 이 순서만은 설계 1.4의 "권한 → 본문"과 반대다.
- **블록 삭제는 블록 종류와 무관하게 카드 관리 권한을 요구한다**(`:372`). 댓글은 사용자 결정으로 바뀌었다: 자기 댓글은 댓글 권한으로 고치고(14:25 KST) 지우고 자기가 지운 것을 되살리며(22:13 KST, O4), 남의 댓글과 남이 지운 자기 댓글은 관리자만(`ManageOthersComments`) 고치고 지우고 되살린다. 댓글을 다른 종류로 바꾸거나 옮기는 것은 편집자부터다(`api::access`). 원본과 달리 댓글은 같은 보드의 카드 위에만 있고 아래에 블록이 없다(B1, 저장소가 400으로 막는다).
- **묶음 패치**(`:626-663`): 경로의 보드를 무시한다. 없는 블록 ID는 404가 아니라 403, 권한 없는 블록은 403 `access denied to make board changesa`(끝의 `a` 포함), 같은 ID 두 번은 404. 팀 ID를 경로에서 읽는데 경로에 팀이 없어 브로드캐스트가 팀 `""`로 나가고, 그래서 팀 구독자는 묶음 패치를 받지 못한다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 경로의 보드에서 카드 관리 권한을 먼저 확인한다(볼 수 없으면 404, 권한이 없으면 403 `access denied to make board changes`). 블록은 모두 그 보드의 것이어야 하고 아니면(없는 블록 포함) 404 `block not found`다. 같은 ID 두 번과 개수가 안 맞는 요청은 400이다(전에는 패치가 모자라면 500, 남으면 무시). 브로드캐스트는 보드의 팀으로 나가 팀 구독자가 받는다(`blocks::block_changes_are_broadcast`).
- **undelete의 거절 문구는 `access denied to modify board members`다**(`:465`). 삭제되지 않은 블록의 undelete는 200과 그 블록. → **S6에서 정리됨(2026-11-10 18:37 KST):** 문구는 `access denied to make board changes`다. 볼 수 없는 보드는 404. 휴지통에 없는 블록(삭제되지 않은 블록도)의 undelete도 404다.
- **블록 목록의 `type`은 적힌 그대로 비교한다**(`Card`는 아무것도 못 찾는다). `type=unknown`은 필터가 아니고, `all`과 `block_id`가 다른 필터보다 우선한다. `page`/`per_page`는 무시된다.
- **`disable_notify`는 standalone에서 로그 한 줄만 막는다.** 브로드캐스트와 웹훅은 그대로 나간다.
- **웹훅은 삽입·패치·묶음 패치·undelete에서 호출되고 삭제·복제에서는 호출되지 않는다**(코드로 확인, 오라클 미확인).
- **시스템 카테고리 삭제는 4xx가 아니라 500이다**(`server/app/category.go:13,119`). `ErrCannotUpdateSystemCategory`는 쓰이지 않아, 시스템 카테고리의 수정은 성공하고 이름만 유지된다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 403 `a system category cannot be deleted`다. 시스템 카테고리의 수정이 성공하고 이름만 유지되는 것은 그대로다.
- **reorder 거절(개수 불일치, 모르는 ID)은 500이다**(`category.go:210-246`, `category_boards.go:212-263`). 개수와 포함 여부만 보므로 하나를 두 번 적고 하나를 빼도 통과한다. 받아들여진 빈 reorder의 응답은 `null`이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 400이다. 받아들여진 빈 reorder의 응답은 `[]`이고 `null` 본문은 400이다. 개수와 포함 여부만 보는 것은 그대로다.
- **`POST …/categories/{id}/boards/{id}`의 응답은 `application/json`이라 표시된 맨 글자 `ok`다**(`server/api/categories.go:391`). 보드가 실제로 있는지는 보지 않는다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 응답은 `{}`다. 자기 카테고리여야 하고(아니면 404 `category not found`) 볼 수 있는 보드여야 한다(아니면 404 `board not found`).
- **hide/unhide는 아무것도 확인하지 않는다.** 없는 보드나 카테고리에도 200 `{}`이고 브로드캐스트도 나간다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 자기 카테고리와 볼 수 있는 보드를 확인한다(둘 다 404). 그 카테고리에 놓이지 않은 보드에 대해서는 전처럼 200이고 아무것도 바뀌지 않는다.
- **기본 카테고리는 이름만으로 알아본다.** 사용자가 만든 "Boards" 카테고리가 있으면 시스템 카테고리가 생기지 않는다. 클라이언트가 `system` 타입 카테고리를 만들거나 `id`·`createAt`·`deleteAt`을 정할 수 있다(이미 있는 `id`는 500). 목록의 `sortOrder`는 항상 0이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 클라이언트는 `id`를 정할 수 없다(400. 처음에는 이미 있는 `id`에 409를 답했는데, 그 답이 남의 카테고리 ID의 존재를 알려 줘서 리뷰 뒤에 바꿨다). `createAt`·`type` 등은 그대로다.
- **`reorder`는 예약어가 아니다.** `DELETE …/categories/reorder`는 ID가 `reorder`인 카테고리를 가리킨다. → **S6에서 정리됨(2026-11-10 19:19 KST):** 예약어다. ID는 서버가 만들고 그 낱말을 만들지 않는다. 그 경로의 다른 메서드는 405다.
- **카테고리·공유 라우트의 잘못된 본문은 모두 500이다.** → **S6에서 정리됨(2026-11-10 18:37 KST):** 400이다.
- **공유 기록이 없는 보드의 `GET …/sharing`은 404 `sql: no rows in result set`이다.** 공유가 꺼진 설정에서의 `POST`는 500, `GET`은 설정을 보지 않는다(코드로 확인). 권한 없는 사용자·비멤버·없는 보드는 모두 403 `access denied to sharing the board`. → **S6에서 정리됨(2026-11-10 18:37 KST):** 기록이 없으면 200 `{"enabled": false}`다. 공유가 꺼진 설정에서의 `POST`는 403 `reason: sharing_disabled`다. 비공개 보드의 비멤버와 없는 보드는 404 `board not found`, 볼 수는 있지만 공유 권한이 없으면 403이다.
- **`GET /teams/{team}/boards/search`는 경로의 팀을 무시한다**(`server/api/search.go:171`). 모르는 `field`는 `q`가 비어도 400. `/boards/search`는 `field`를 무시한다.
- **`GET /teams/{team}/channels`와 `/boards/search/linkable`은 세션 검사 뒤 501 `not permitted in standalone mode`.** → **S6에서 정리됨(2026-11-10 18:37 KST):** 라우트를 없앴다(결정 4). 404.
- **`null` 본문은 500**(`POST /boards`, `PATCH /boards/{id}`, 멤버 `POST`/`PUT`, `DELETE /boards-and-blocks`). 그 밖의 잘못된 본문은 그 라우트들에서 400이다. `POST`·`PATCH /boards-and-blocks`는 디코드할 수 없는 본문이 모두 500이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** `null` 본문은 어느 라우트에서나 400이다. `POST`·`PATCH /boards-and-blocks`의 디코드할 수 없는 본문도 400이다.
- **`POST /boards`는 유효성보다 권한을 먼저 본다.** 팀이 없으면 403(`access denied to create private boards` 또는 `…public boards`), 타입 오류는 그 뒤에 400 `invalid-board-type`. `id`를 넣어 보내면 500이다(`server/app/boards.go:234`). → **S6에서 정리됨(2026-11-10 18:37 KST):** `id`를 넣어 보내면 400(`field: id`)이다. 권한이 먼저인 순서는 설계 1.4와 같아 그대로다.
- **`PUT /boards/{id}/members/{user}`는 권한보다 사용자 조회가 먼저다.** 없는 사용자는 관리자가 아니어도 404 `{user} not found`. 멤버가 아닌 사용자나 삭제된 보드에는 200 `null`(`server/api/members.go:441-452`). → **S6에서 정리됨(2026-11-10 18:37 KST):** 보드를 볼 수 있는지(404) → 역할 관리 권한(403) → 본문(400) → 멤버십 순서다. 멤버가 아닌 사용자와 없는 사용자는 둘 다 404 `member not found`다(`boards::update_member_checks`).
- **`POST …/members`는 사용자가 실제로 있는지 보지 않는다.** 없는 사용자 ID의 멤버십(과 기본 카테고리)이 만들어진다. 본문에서는 `userId`와 역할 플래그 넷만 읽는다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 없는 사용자는 400 `no such user`(`field: userId`)다.
- **`POST /boards-and-blocks`의 첫 보드는 아무것도 검사받지 않는다**(`:101-104`). `DELETE`는 "보드가 하나 이상" 검사를 블록 검사 뒤 맨 마지막에 한다. `PATCH`는 보드 조회보다 권한이 먼저라 없는 보드가 403이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 모든 보드가 같은 검사를 받는다(ID 없는 보드는 첫 번째여도 400). `DELETE`는 "보드가 하나 이상"을 먼저 본다. `PATCH`·`DELETE`의 없는 보드와 볼 수 없는 보드는 404, 없는 블록과 나열하지 않은 보드의 블록은 같은 400이다.
- **블록이 없거나 댓글만 있는 보드의 복제는 500이다.** `asTemplate`은 정확히 `true`일 때만 참이다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 블록 없는 보드로 복제된다(`boards::duplicate_board_without_blocks`, 스토어 `duplicate_board_copies_a_board_without_blocks`). 그런 보드를 내보낸 아카이브도 가져올 수 있다(`export::export_of_a_board_without_blocks_round_trips`). 요청은 옳으므로 400으로 둘 수 없었다. `asTemplate`은 그대로다.
- **삭제된 보드의 멤버십은 남는다.** undelete가 그 멤버십으로 동작하고, 그 상태의 leave는 404다.
- **사용자 행이 없는 세션**은 원본이 `userIsGuest`를 부르는 자리에서 404 `{user} not found`를 받는다(오라클에서 로그인한 사용자의 행을 지워 확인).
- **`GET /boards/{id}/metadata`는 항상 501 `appropriate license required`다.** 보드나 권한 검사보다 먼저다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 라우트를 없앴다(결정 4). 404.
- **`POST /boards/{id}/join?allow_admin`은 효과가 없다.** `ManageTeam`을 가진 사용자가 없다. 비공개 보드는 항상 403. → **S6에서 정리됨(2026-11-10 18:37 KST):** 비공개 보드는 비멤버에게 404다(멤버는 이미 멤버다). `allow_admin`은 여전히 읽지 않는다.
- **viewer도 볼 수 있는 보드를 복제할 수 있다**(S5 권한 표로 확인).
- **`/cards/{id}`의 없는 블록·카드가 아닌 블록은 404가 아니라 400이고 권한 검사보다 먼저다**(`:279-284`, `:356-361`). → **S6에서 정리됨(2026-11-10 18:37 KST):** 없는 블록, 카드가 아닌 블록, 읽을 수 없는 보드의 카드가 모두 404 `card not found`다(결정 대기 S11).
- **카드 생성의 검사 순서:** 디코드(400) → 권한(403) → 보드 불일치(400 `Board IDs do not match`) → 검증(400). 패치는 카드 조회(400) → 권한(403) → 디코드(400). `null` 본문은 500. → **S6에서 정리됨(2026-11-10 18:37 KST):** 생성은 보드(404) → 권한(403) → 본문(400) → 검증(400), 패치는 카드(404) → 권한(403) → 본문(400)이다. `null` 본문은 400.
- **패치의 잘못된 아이콘은 500, 생성의 같은 아이콘은 400이다**(`server/app/cards.go:66-69`). → **S6에서 정리됨(2026-11-10 18:37 KST):** 둘 다 400이다.
- **잘못된 카드 블록 하나가 카드 목록 전체를 500으로 만든다**(`server/app/cards.go:54-60`). 원본 웹 앱이 나란히 놓인 콘텐츠를 `contentOrder`의 중첩 목록으로 쓰는데, 그것이 여기에 해당한다. `properties`가 객체가 아닌 경우만은 포트가 읽기에서 견딘다(의도적 차이 46). → **S6에서 정리됨(2026-11-10 18:37 KST):** 목록의 500은 그대로다(저장된 것이 카드가 아닌 것은 요청의 잘못이 아니라 서버 상태다). 그 카드 하나의 조회도 같은 이유로 400이 아니라 500이 됐다. S14의 쓰기 검증이 이런 블록이 새로 생기는 것은 막지 않는다(`contentOrder`의 중첩 목록은 검증 대상이 아니다). 본문 저장을 바꿀 때(설계 3.7) 없어진다. → **정리됨(2026-11-11 13:57 KST):** 종류가 맞지 않는 필드는 읽을 때 빼므로 목록이 실패하지 않는다(tracer-bullet 12).
- **카드 패치의 `updatedProperties`는 속성 전체를 바꾼다**(`server/model/card.go:306-308`). → **정리됨(2026-11-11 13:08 KST):** 보낸 값만 합치고, 지울 속성은 `deletedProperties`로 받는다(사용자 결정 2026-11-11 11:05 KST).
- **카드 페이징:** 기본 `page=0`, `per_page=100`. 0페이지의 `per_page <= 0`은 전체, 음수 페이지는 `[]`, `per_page` 상한 없음.
- **콘텐츠 블록 이동:** 두 블록 조회(누구에게나 404) → `where`(400, 대소문자 구분) → 옮기는 블록 보드의 권한 순서다. 자기 자신으로의 이동은 400 `source block X not found`. 순서가 그대로여도 카드는 패치된다(`updateAt`, 브로드캐스트, 웹훅). → **S6에서 정리됨(2026-11-10 18:37 KST):** 옮기는 블록(404) → 그 보드의 권한(읽을 수 없으면 404 `block not found`, 권한이 없으면 403) → 놓을 자리의 블록(같은 식) → `where`(400) 순서다. 부모가 없는 블록은 400 `the block is not content of a card`, 자기 자신으로의 이동은 400 `a block cannot be moved next to itself`, 순서에 없는 블록은 400 `block X is not in the content order of its card`다.
- **구독:** 잘못된 본문은 500, `null` 본문은 400 `missing block id`. `blockType`은 비어 있지만 않으면 되고 블록과 비교하지 않는다. 보드는 구독할 수 없다. 두 번 구독하면 응답의 `createAt`은 새 값이지만 저장된 값은 처음 것이다. 없는 구독의 해지는 404. 남의 목록·해지는 403 `access denied`. 구독은 블록이 지워져도 남는다. (S6: 멤버가 보드에서 빠지면 그 보드의 구독은 끝난다. 보안 결정 대기 S12.) → **S6에서 정리됨(2026-11-10 18:37 KST):** 잘못된 본문과 `null`은 400. `subscriberType`은 `user`만 받는다(`channel`은 400). 남을 위한 구독은 403(`field: subscriberId`), 없는 블록과 읽을 수 없는 보드의 블록은 404 `block not found`, 남의 목록은 404 `subscriber not found`, 남의 것이나 없는 구독의 해지는 404 `subscription not found`다.
- **없는 파일의 다운로드는 404가 아니라 500, 없는 파일의 정보는 200 `null`이다**(오라클 확인). 이름의 첫 글자가 여러 바이트 문자인 파일(템플릿 보드는 클라이언트가 준 이름을 그대로 쓴다)은 기록 없이 `<팀>/<보드>/<이름>` 경로로 찾는다. → **S6에서 정리됨(2026-11-10 18:37 KST):** 둘 다 404 `file not found`다. 다른 보드의 파일도 같은 답이라 둘을 구별할 수 없다. 기록 없이 찾는 파일(첫 글자가 여러 바이트인 이름)은 다운로드는 되지만 정보는 404다. 파일 기록을 고유 키로 찾게 되면(설계 3.1) 없어진다.
- **내보내기는 없는 파일을 0바이트 항목으로 쓰고 계속한다**(오라클 확인).
- **가져오기에서 multipart 필드를 못 읽으면 오류 문구가 200의 본문으로 나간다**(`server/api/archive.go`). → **S6에서 정리됨(2026-11-10 18:37 KST):** 400 `bad_request`다. 아카이브가 아닌 파일, 빈 ZIP, 모르는 버전도 500이 아니라 400이다.
- **템플릿 보드에 올린 파일은 클라이언트가 준 이름을 그대로 쓴다**(`<team>/<board>/<name>`).
- **옛 형식 보드 블록의 `templateVer`는 무시된다.** Go의 `v.(int)` 단언이 JSON 숫자(float)에는 성공하지 않는다. → S7에서 옛 형식(판 1)의 가져오기를 걷어 내며 없어졌다.
- **로그인한 사용자는 누구나 아카이브를 가져올 수 있다. 보드 내보내기는 viewer를 포함한 멤버 모두 가능하다.**
- **파일 복사가 실패해도 복제는 끝난다**(`server/app/files.go:186-190`). 복사 실패는 로그만 남고, 복제된 이미지·첨부 블록의 `fileId`는 빈 값이 된다. 빈 `fileId`를 가진 블록이 있는 보드의 복제도 200이다(오라클 확인).
- **아카이브의 `boardMember` 줄은 역할 플래그까지 그대로 가져온다.** 아카이브에 다른 사용자를 admin으로 적으면 가져온 보드의 admin이 된다(오라클 확인). 가져온 사용자는 그 보드의 admin이라 멤버 라우트로도 같은 일을 할 수 있으므로 권한 상승은 아니다. 플러그인 저장소는 PR #207에서 이 플래그를 지운다.
- **빈 파일에 대한 Range 요청은 무시되고 200이다**(원본의 `http.ServeContent`와 같다).
- **`board.jsonl`과 옛 형식 아카이브는 70MB까지만 읽는다**(`server/app/import.go:24`). → S7부터 옛 형식은 읽지 않고, `board.jsonl`의 한도는 그대로다.
- **복제 응답의 블록은 복사 전의 `fileId`를 담고 있다.** 새 파일 이름은 저장된 블록에만 쓰인다(`server/app/files.go:185-220`).

## 리뷰 (rust-review)

서버 크레이트는 두 번에 나눠 `rust-review`를 돌렸다(둘 다 Fable 5.2, 읽기 전용, 2026-11-10). 첫 번째는 파일 영역을 뺀 약 11,000줄, 두 번째는 파일·아카이브·템플릿·온보딩 영역 약 2,400줄이다. 두 번째 영역은 다른 모델이 이어 쓴 부분이라 전 줄을 읽게 했다.

### 첫 번째 (서버 전체, 파일 영역 제외)

| 등급 | 지적 | 처리 |
|---|---|---|
| Must | bcrypt가 비동기 워커 스레드에서 돈다 | 반영. 블로킹 풀로 옮기고 프로세서 수만큼만 동시에 돈다 |
| Must | `/`로 시작하지 않는 base path가 시작 때 패닉 | 반영. 시작 오류로 바꿨다 |
| Should | 클라이언트가 끊기면 핸들러가 쓰기 사이에서 멈춘다 | 반영(의도적 차이 58) |
| Should | 느린 WebSocket 연결을 "끊는다"고 했지만 닫히지 않는다 | 반영. 취소 토큰으로 읽기 쪽을 끝내고, 쓰기 쪽은 5초 뒤 중단한다 |
| Should | 종료 때 연결 대기에 시간 제한이 없고 두 번째 Ctrl-C가 무시된다 | 반영(의도적 차이 59) |
| Should | 인증 경로의 스토어 실패가 로그 없이 401이 된다 | 반영(의도적 차이 62) |
| Should | `root_workspace`가 읽기 실패를 "워크스페이스 없음"으로 보고 가입 토큰을 바꾼다 | 반영(의도적 차이 61) |
| Should | 빈 single-user 토큰이 모든 요청을 통과시킨다 | 반영(의도적 차이 60) |
| Should | `api/*.rs`에 거의 같은 헬퍼가 중복 | 반영. `api/access.rs`(권한 확인), `api/body.rs`(본문 디코딩), `change_notification`·`duplicate_as`는 `api/mod.rs`. 디코더를 정책 enum 하나로 합치는 것까지는 하지 않았다(호출부가 원본의 핸들러별 차이를 이름으로 드러내는 편이 읽기 쉽다) |
| Should | 필요 이상으로 넓은 공개 범위 | 부분 반영. 지적된 세 항목은 `pub(crate)`로 줄였다. `api`·`permissions`·`scheduler`·`ws` 모듈을 비공개로 돌리는 것은 하지 않았다(통합 테스트가 `Server`·`App`의 공개 API로 이 타입들에 닿는다) |
| Should | `lib.rs`에 인라인 모듈 셋 | 반영. `request_log.rs`, `tls.rs`, `admin_socket.rs`(와 Unix가 아닌 플랫폼용 `admin_socket_unsupported.rs`) |
| Should | 브로드캐스트 경로가 듣는 사람이 없어도 스토어를 조회하고 수신자마다 본문을 복사한다 | 부분 반영. 워크스페이스에 듣는 연결이 없으면 조회하지 않고, 본문은 공유한다. 블록 여러 개를 한 번의 멤버 조회로 묶는 것은 하지 않았다(원본처럼 블록마다 따로 브로드캐스트한다) |
| Consider | `null_as_default` 문서, 중복된 공백이 든 오류 문구, 사용자 목록의 반복 조회, 웹훅 로그의 URL, 정적 파일 이름의 이중 디코딩 | 반영 |
| Consider | 설정 값의 enum화, 저장소 쓰기의 원자성, WebSocket 메시지 크기 제한과 구독 자료구조, 테스트 DB 스키마 정리, 그 밖의 작은 지적 | 반영하지 않음. 동작에 영향이 없거나(구조), 원본과 달라지는 변경이다(메시지 크기 제한) |

### 두 번째 (파일, 아카이브, 템플릿, 온보딩)

| 등급 | 지적 | 처리 |
|---|---|---|
| Must | 첫 글자가 여러 바이트 문자인 파일 이름이 모든 읽기에서 500 | 반영. 원본처럼 기록 없음으로 처리한다. 테스트 `files::a_template_file_whose_name_begins_with_a_wide_character_is_served` |
| Must | 빈 파일의 접미 Range가 정수 언더플로 | 반영. 빈 파일의 Range는 원본처럼 무시한다. 테스트 `files::a_range_on_an_empty_file_is_ignored` |
| Should | ZIP 압축 해제와 JSONL 해석이 비동기 워커에서 돈다 | 반영. 둘 다 블로킹 풀에서 한다. 내보내기의 압축은 그대로다 |
| Should | `board.jsonl`의 한도가 원본보다 느슨하다 | 반영. 원본의 70MB |
| Should | 도중에 실패한 템플릿 초기화가 다시 시도되지 않는다 | 반영(의도적 차이 63) |
| Should | 온보딩이 welcome 보드를 제목만으로 찾는다 | 결정 대기 S18(원본에서 재현되지 않았다). S6에서 정리됨 |
| Should | 실패한 S3 쓰기가 multipart 업로드를 열어 둔다 | 반영 |
| Should | `App::move_file`, `App::get_file_reader`는 닿지 않는 위험한 코드 | 반영. 지웠다(의도적 차이 64). 저장 계층의 `rename`은 남겼다 |
| Should | 이미지 블록마다 대상 보드를 다시 읽고 복제한다 | 반영 |
| Should | 올린 아카이브를 한 번 더 복사한다 | 반영 |
| Should | 코드와 어긋난 문서와 시그니처 | 반영 |
| Consider | 확장자에 `/`가 든 `fileId`로 만든 복사본이 보드에 묶이지 않는다, 이름 없는 아카이브 항목 | 반영 |
| Consider | 이상한 `fileId` 하나로 내보내기가 실패, 가져오기 도중 실패 시 보드 정리, 아카이브 전체 크기 한도, 업로드·내보내기의 스트리밍, 저장 계층의 삭제 연산, `If-Match` 등 다운로드 세부 | 반영하지 않음. 원본과 같은 동작이거나(앞의 둘), 결정 대기 S17·의도적 차이 56에 적은 구조 변경이다 |

### S6의 리뷰 (2026-11-10 19:19 KST)

S6의 A·C와 설계 1단계·한도를 리뷰한 결과와 처리는 루트 `PORTING.md`의 "S6" 절 F에 있다. 이 문서의 "보존한 quirk"와 "보안 결정 대기"에 적은 동작 가운데 리뷰로 다시 바뀐 것: 카테고리는 만들 때 ID를 정할 수 없다(1단계에서 넣은 409는 없다). `reorder`는 예약어다(그 경로의 다른 메서드는 405). 템플릿은 join할 수 없다. 세션 토큰 없는 요청의 본문 한도는 어느 라우트에서나 64 KiB다. 속성 정의의 중복 ID는 400이다.

## 검증 (S5, 2026-11-10)

(S6: 아래에 나오는 `scripts/` 아래의 스크립트는 주 브랜치에서 걷어 냈다. 태그 `parity-s5`에 남아 있다.)

rustc 1.99.1, SQLite(번들), PostgreSQL 18.6(컨테이너 `hafgufa-pg`), S3 호환 서버 SeaweedFS(컨테이너 `hafgufa-s3`). 원본은 `a84bbb65`를 Go 1.21.8로 빌드한 오라클이다.

| 검증 | 명령 | 결과 |
|---|---|---|
| 형식 | `cargo fmt --all --check` | 통과 |
| 린트 | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 통과 |
| 테스트 (SQLite) | `cargo test --workspace` | 모델 69, 스토어 121, 서버 512(단위 78, API 423, 저장소 2, 웹 7, 웹훅 2) 통과 |
| 테스트 (PostgreSQL) | `FVOCI_STORE_TEST_DB_TYPE=postgres FVOCI_STORE_TEST_CONN_STRING=… cargo test -p fvoci-store -p fvoci-server` | 스토어 121, 서버 512 통과 |
| S3 백엔드 | `FVOCI_S3_TEST_ENDPOINT=localhost:18333 FVOCI_S3_TEST_BUCKET=fvoci-test cargo test -p fvoci-server --test storage -- --ignored` | 1 통과 (SeaweedFS) |
| 문서 | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | 통과 |
| 차등 비교 | `scripts/differential/run.py --left <원본> --right <포트>` | 시나리오 15개, 382단계. 예상 밖 차이 0건, 등록된 차이 21건 |
| 권한 표 | `scripts/differential/permissions.py --left <원본> --right <포트>` | 라우트 53개 × 역할 6개 × 보드 종류 2개 = 636칸. 예상 밖 차이 0건, 등록된 차이 11건 |
| DB 왕복 | `scripts/differential/roundtrip.sh sqlite`, `… postgres <URL>` | 원본→포트, 포트→원본을 SQLite와 PostgreSQL에서. 방향마다 관찰 22건, 차이 0건(사용자 설정 1건은 결정 항목 1로 등록) |
| 브라우저 종단 | `scripts/e2e-cypress.sh rust-s5 target/debug/fvoci-server` | 원본 React 번들 + 포트에서 원본 Cypress 스펙 6개, 테스트 11개 모두 통과(S0의 원본 기준선과 같다) |
| Docker 이미지 | `docker build -f docker/fvoci/Dockerfile -t fvoci-server:s5 .` | 빌드 성공(208MB, Debian 13 기반). 컨테이너를 띄워 `/ping`, 가입·로그인, 내장 템플릿 13개, `index.html`, SIGTERM 정상 종료를 확인 |
| 제3자 고지 | `cargo about generate --manifest-path crates/server/Cargo.toml -o NOTICE about.hbs` | `NOTICE` 생성(서버 바이너리에 들어가는 크레이트의 라이선스 전문) |
| 라이선스 | `cargo deny check` | advisories, bans, licenses, sources 모두 ok(RUSTSEC-2026-0459는 `deny.toml`에 사유와 함께 예외) |
| 이름 | `grep -rniE "<옛 이름 둘>" crates/` | 0건 |

등록된 차이의 목록과 사유는 `scripts/differential/expected.py`에 있고, 모두 이 문서의 의도적 차이·보안 수정 표나 루트 `PORTING.md`의 결정 항목을 가리킨다. 차등 비교의 시나리오와 권한 표는 먼저 원본 서버 두 개끼리 돌려 차이 0건임을 확인한 뒤에 썼다.

비교 범위에 대한 메모:

- 이미지·첨부 블록 삭제: 원본은 접두사 없는 `FileInfo` 테이블을 갱신하려다 500을 답하고 블록이 남는다(`sqlstore/blocks.go:430`, `:987`). 포트는 접두사가 붙은 `file_info`의 삭제 시각을 갱신하고 블록을 지운다(스토어 문서의 의도적 차이). 차등 비교에 등록했다.
- 캐시 헤더, WebSocket ping 같은 CDN 대응 항목은 차등 비교 대상이 아니고 포트의 테스트로만 확인했다.

이미지를 처음 띄웠을 때 찾은 것 둘:

- Debian 12 기반에서는 TLS 라이브러리(`aws-lc-sys`)의 사전 빌드 객체가 요구하는 C 라이브러리 심볼이 없어 링크가 실패한다. 빌드·실행 이미지를 Debian 13으로 했다.
- 인증 기관 인증서가 없는 시스템에서 웹훅용 HTTP 클라이언트를 만들다 서버가 시작 때 패닉했다. 웹훅 주소가 없으면 클라이언트를 만들지 않고, 있는데 만들 수 없으면 시작 오류로 알린다. 이미지에는 `ca-certificates`를 넣었다.

### 미검증

- **실제 AWS S3와 Cloudflare R2.** S3 백엔드는 SeaweedFS로만 확인했다. IAM·STS 자격 증명, SSE, R2는 자격 증명이 없어 돌리지 못했다.
- **CDN 경유.** CloudFront나 Cloudflare를 실제로 거친 확인은 하지 않았다. `docs/fvoci-cdn.md`의 설정 예시는 헤더 테스트와 벤더 문서에 근거한다.
- **GitHub Actions 워크플로**(`.github/workflows/fvoci-ci.yml`)는 푸시가 필요해 실행하지 않았다.
- **원본 Go 테스트 스위트**는 실행하지 않았다. 원본의 동작은 오라클에 요청을 보내 확인했다.
- **플러그인 저장소 점검 목록의 일부 항목**은 오라클에 공격을 재현하지 않고 코드로만 판단했다(압축 폭탄, 온보딩 템플릿 가로채기). 각 항목에 그렇게 적었다.
- **수동 확인**(두 탭 실시간 동기화)은 하지 않았다. WebSocket 수신자 선정은 테스트와 Cypress로만 확인했다.

