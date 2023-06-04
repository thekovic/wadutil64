cmake_minimum_required(VERSION 3.5)

project(wadutil64 LANGUAGES CXX)

if (MSVC)
    # Warning level 4
    add_compile_options(/W4)
else()
    # Additional warnings
    add_compile_options(-Wall -Wextra)
endif()

set(EXECUTABLE_OUTPUT_PATH ..)

add_executable(wadutil64
    main.cpp
	decodes.cpp
	encodes.cpp
)
