cmake_minimum_required(VERSION 3.5)

project(SP LANGUAGES C)
set(CMAKE_C_FLAGS "-Wall -Wextra -pedantic")
set(EXECUTABLE_OUTPUT_PATH ..)

add_executable(Doom64Compressor
    main.c
	decodes.c
	w_wad.c
)
