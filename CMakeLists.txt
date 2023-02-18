cmake_minimum_required(VERSION 3.0)

project(dpdk-project C CXX)
set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)

find_package(PkgConfig REQUIRED)
pkg_check_modules(DPDK IMPORTED_TARGET REQUIRED libdpdk)

set(SOURCES
  src/main.cpp
  rust/bindings.h
)

add_executable(dpdkproj ${SOURCES})
target_include_directories(dpdkproj PRIVATE rust)

set(RUST_TARGET_DEBUG_DIR ${CMAKE_CURRENT_SOURCE_DIR}/rust/target/debug)
set(RUST_TARGET_RELEASE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/rust/target/release)
file(GLOB_RECURSE RUST_SOURCES rust/src/*.rs rust/Cargo.toml rust/Cargo.lock)

add_custom_command(
  OUTPUT ${RUST_TARGET_DEBUG_DIR}/libdpdk_project.a
  COMMAND cargo build
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/rust
  DEPENDS ${RUST_SOURCES}
)

add_custom_command(
  OUTPUT ${RUST_TARGET_RELEASE_DIR}/libdpdk_project.a rust/bindings.h
  COMMAND cargo build --release
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/rust
  DEPENDS ${RUST_SOURCES}
)

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  set(RUST_TARGET_DIR ${RUST_TARGET_DEBUG_DIR})
else()
  set(RUST_TARGET_DIR ${RUST_TARGET_RELEASE_DIR})
endif()

add_custom_target(rust
  ALL
  DEPENDS ${RUST_TARGET_DIR}/libdpdk_project.a
)

set(RUST_LIBRARY ${RUST_TARGET_DIR}/libdpdk_project.a)

target_link_libraries(dpdkproj PkgConfig::DPDK ${RUST_LIBRARY})
