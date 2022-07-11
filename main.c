#include "doomdef.h"

void choose_decode_mode(byte *decode_mode, char *lump_name)
{
    char MAP01_name[6] = "MAP01";
    MAP01_name[0] += 0x80;
    if (!strcmp(lump_name, "T_START"))
    {
        *decode_mode = dec_d64;
    }
    else if (!strcmp(lump_name, "T_END"))
    {
        *decode_mode = dec_jag;
    }
    else if (!strcmp(lump_name, MAP01_name))
    {
        *decode_mode = dec_d64;
    }
}

int main(int argc, char **argv)
{
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
    printf("WAD name: %s\n", argv[2]);
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
    
    byte decode_mode = 1;
    size_t total_size = 12;
    for (int i = 1; i < wad_header.numlumps - 1; ++i)
    {
        size_t lump_size = lump_directory[i+1].filepos - lump_directory[i].filepos;
        byte *lump_data = malloc(lump_size);
        if (!lump_data)
        {
            printf("ERROR: Could not read WAD lump %d.", i);
            return EXIT_FAILURE;
        }
        byte *true_lump_data = malloc(lump_directory[i].size);
        if (!true_lump_data)
        {
            printf("ERROR: Could not decompress WAD lump %d.", i);
            return EXIT_FAILURE;
        }

        choose_decode_mode(&decode_mode, lump_directory[i].name);

        fseek(wad, lump_directory[i].filepos, SEEK_SET);
        fread(lump_data, lump_size, 1, wad);

        if (lump_directory[i].name[0] & 0x80)
        {
            lump_directory[i].name[0] -= 0x80;
            if (decode_mode == dec_jag)
            {
                DecodeJaguar(lump_data, true_lump_data);
            }
            else if (decode_mode == dec_d64)
            {
                DecodeD64(lump_data, true_lump_data);
            }
            lump_directory[i].filepos = lump_directory[i - 1].filepos + lump_directory[i - 1].size;
            total_size += lump_directory[i].size;
            free(lump_data);
        }
        else
        {
            free(true_lump_data);
            true_lump_data = lump_data;
            lump_directory[i].filepos = lump_directory[i - 1].filepos + lump_directory[i - 1].size;
            total_size += lump_directory[i].size;
        }
        fwrite(true_lump_data, lump_directory[i].size, 1, output);
        free(true_lump_data);
    }
    
    lump_directory[wad_header.numlumps - 1].filepos
        = lump_directory[wad_header.numlumps - 2].filepos + lump_directory[wad_header.numlumps - 2].size;
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output);

    wad_header.infotableofs = total_size;
    fseek(output, 0, SEEK_SET);
    fwrite(&wad_header, sizeof(wadinfo_t), 1, output);
    
    fclose(output);
    fclose(wad);
    return EXIT_SUCCESS;
}