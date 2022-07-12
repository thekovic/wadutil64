#include "doomdef.h"

typedef enum
{
    DECOMPRESS,
    COMPRESS
} d64compressor_mode;

static char input_file_name[128];
static char output_file_name[128];

void choose_decode_mode(byte *decode_mode, char *lump_name)
{
    char MAP01_name[6] = "MAP01";
    MAP01_name[0] += 0x80;
    if (!strcmp(lump_name, "T_START"))
    {
        *decode_mode = DECODE_D64;
    }
    else if (!strcmp(lump_name, "T_END"))
    {
        *decode_mode = DECODE_JAGUAR;
    }
    else if (!strcmp(lump_name, MAP01_name))
    {
        *decode_mode = DECODE_D64;
    }
}

void d64compressor_help()
{
    printf("Improper arguments!\n");
    printf("USAGE:\n");
    printf("    Decompression: Doom64Compressor.exe -d DOOM64.WAD\n");
    printf("    Compression: Doom64Compressor.exe -c DOOM64.WAD\n");
}

void decompress_WAD(FILE *input_WAD, FILE *output_WAD)
{
    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, input_WAD);
    printf("WAD name: %s\n", input_file_name);
    printf("Number of lumps: %d, Address to directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    lumpinfo_t *lump_directory = malloc(wad_header.numlumps * sizeof(lumpinfo_t));
    if (!lump_directory)
    {
        printf("ERROR: Could not read WAD lumps.");
        exit(EXIT_FAILURE);
    }
    fseek(input_WAD, wad_header.infotableofs, SEEK_SET);

    for (int i = 0; i < wad_header.numlumps; ++i)
    {
        fread(lump_directory + i, sizeof(lumpinfo_t), 1, input_WAD);
    }

    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);
    
    byte decode_mode = 1;
    size_t total_size = 12;
    for (int i = 1; i < wad_header.numlumps - 1; ++i)
    {
        size_t lump_size = lump_directory[i+1].filepos - lump_directory[i].filepos;
        byte *lump_data = malloc(lump_size);
        if (!lump_data)
        {
            printf("ERROR: Could not read WAD lump %d.", i);
            exit(EXIT_FAILURE);
        }
        byte *true_lump_data = malloc(lump_directory[i].size);
        if (!true_lump_data)
        {
            printf("ERROR: Could not decompress WAD lump %d.", i);
            exit(EXIT_FAILURE);
        }

        choose_decode_mode(&decode_mode, lump_directory[i].name);

        fseek(input_WAD, lump_directory[i].filepos, SEEK_SET);
        fread(lump_data, lump_size, 1, input_WAD);

        if (lump_directory[i].name[0] & 0x80)
        {
            lump_directory[i].name[0] -= 0x80;
            if (decode_mode == DECODE_JAGUAR)
            {
                DecodeJaguar(lump_data, true_lump_data);
            }
            else if (decode_mode == DECODE_D64)
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
        fwrite(true_lump_data, lump_directory[i].size, 1, output_WAD);
        free(true_lump_data);
    }
    
    lump_directory[wad_header.numlumps - 1].filepos
        = lump_directory[wad_header.numlumps - 2].filepos + lump_directory[wad_header.numlumps - 2].size;
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output_WAD);

    wad_header.infotableofs = total_size;
    fseek(output_WAD, 0, SEEK_SET);
    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);
}

void compress_WAD(FILE *input_WAD, FILE *output_WAD)
{
    printf("TODO: WAD COMPRESSION!!!\n");
}

int main(int argc, char **argv)
{
    if (argc != 3)
    {
        d64compressor_help();
        return EXIT_FAILURE;
    }

    byte program_mode;
    if (argv[1][1] == 'd')
    {
        program_mode = DECOMPRESS;
        printf("Decompression mode enabled!\n");
    }
    else if (argv[1][1] == 'c')
    {
        program_mode = COMPRESS;
        printf("Compression mode enabled!\n");
    }
    else
    {
        d64compressor_help();
        return EXIT_FAILURE;
    }

    strncpy(input_file_name, argv[2], 128);
    FILE *wad = fopen(input_file_name, "rb");
    if (!wad)
    {
        printf("ERROR: WAD file %s not found!\n", input_file_name);
        return EXIT_FAILURE;
    }

    strncpy(output_file_name, input_file_name, 128);
    output_file_name[strlen(output_file_name) - 4] = 0;
    if (program_mode == DECOMPRESS)
    {
        strcat(output_file_name, "_decomp.WAD");
    }
    else
    {
        strcat(output_file_name, "_comp.WAD");
    }
    FILE *output = fopen(output_file_name, "wb");
    if (!output)
    {
        printf("ERROR: Could not write decompressed WAD!\n");
        return EXIT_FAILURE;
    }

    if (program_mode == DECOMPRESS)
    {
        decompress_WAD(wad, output);
    }
    else
    {
        compress_WAD(wad, output);
    }
    
    fclose(output);
    fclose(wad);
    return EXIT_SUCCESS;
}