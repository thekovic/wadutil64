#include "doomdef.h"

int main(int argc, char **argv) {
    if (argc != 3)
    {
        printf("Improper arguments!\n");
        printf("USAGE:\n");
        printf("    Decompression: Doom64Compressor.exe -d DOOM64.WAD\n");
        printf("    Compression: Doom64Compressor.exe -c DOOM64.WAD\n");
        return EXIT_FAILURE;
    }

    FILE *wad = fopen(argv[2], "rb");
    if (!wad)
    {
        printf("ERROR: WAD file %s not found!\n", argv[2]);
        return EXIT_FAILURE;
    }

    char output_name[128];
    strncpy(output_name, argv[2], 128);
    output_name[strlen(output_name) - 4] = 0;
    strcat(output_name, "_decomp.WAD");

    FILE *output = fopen(output_name, "wb");
    if (!output)
    {
        printf("ERROR: Could not write decompressed WAD!\n");
        return EXIT_FAILURE;
    }

    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, wad);
    printf("WAD name: %s", argv[2]);
    printf("Number of lumps: %d, Address to directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    lumpinfo_t *lump_directory = malloc(wad_header.numlumps * sizeof(lumpinfo_t));
    if (!lump_directory)
    {
        printf("ERROR: Could not read WAD lumps.");
        return EXIT_FAILURE;
    }
    fseek(wad, wad_header.infotableofs, SEEK_SET);

    for (int i = 0; i < wad_header.numlumps; ++i)
    {
        fread(lump_directory + i, sizeof(lumpinfo_t), 1, wad);
    }

    fwrite(&wad_header, sizeof(wadinfo_t), 1, output);
    
    for (int i = 1; i < wad_header.numlumps - 1; ++i)
    {
        size_t lump_size = lump_directory[i+1].filepos - lump_directory[i].filepos;
        byte *lump_data = malloc(lump_size);
        if (!lump_data)
        {
            printf("ERROR: Could not read WAD lump %d.", i);
            return EXIT_FAILURE;
        }

        fseek(wad, lump_directory[i].filepos, SEEK_SET);
        fread(lump_data, lump_size, 1, wad);
        fwrite(lump_data, lump_size, 1, output);
        free(lump_data);
    }
    
    fseek(wad, wad_header.infotableofs, SEEK_SET);
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output);
    
    fclose(output);
    fclose(wad);
    return EXIT_SUCCESS;
}