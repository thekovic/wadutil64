#include "doomdef.h"

typedef enum
{
    EXTRACT_MODE,
    DECOMPRESS_MODE,
    COMPRESS_MODE,
    PAD_MODE
} wadutil64_mode;

static char input_file_name[128];
static char output_file_name[128];

void choose_decode_mode(byte* decode_mode, char* lump_name)
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

void wadutil64_help()
{
    printf("Improper arguments!\n");
    printf("USAGE:\n");
    printf("    Extraction: wadutil64.exe -e DOOM64_ROM.z64\n");
    printf("    Decompression: wadutil64.exe -d DOOM64.WAD\n");
    printf("    Compression: wadutil64.exe -c DOOM64.WAD\n");
    printf("    Padding: wadutil64.exe -p DOOM64.WAD\n");
}

void extract_WAD(FILE* input_ROM, FILE* output_WAD)
{
    printf("TODO: WAD EXTRACTION!!!\n");
}

void decompress_WAD(FILE* input_WAD, FILE* output_WAD)
{
    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, input_WAD);
    printf("WAD name: %s\n", input_file_name);
    printf("Number of lumps: %d, Address to directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    lumpinfo_t* lump_directory = (lumpinfo_t*) malloc(wad_header.numlumps * sizeof(lumpinfo_t));
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
        byte* lump_data = (byte*) malloc(lump_size);
        if (!lump_data)
        {
            printf("ERROR: Could not read WAD lump %d.", i);
            exit(EXIT_FAILURE);
        }
        byte* true_lump_data = (byte*) malloc(lump_directory[i].size);
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

void compress_WAD(FILE* input_WAD, FILE* output_WAD)
{
    printf("TODO: WAD COMPRESSION!!!\n");
}

void pad_WAD(FILE* input_WAD, FILE* output_WAD)
{
    printf("TODO: WAD PADDING!!!\n");
}

int main(int argc, char** argv)
{
    if (argc != 3)
    {
        wadutil64_help();
        return EXIT_FAILURE;
    }

    // Open input file
    strncpy(input_file_name, argv[2], 128);
    FILE* input_file = fopen(input_file_name, "rb");
    if (!input_file)
    {
        printf("ERROR: Input file %s not found!\n", input_file_name);
        return EXIT_FAILURE;
    }

    // Modify output file name
    strncpy(output_file_name, input_file_name, 128);
    output_file_name[strlen(output_file_name) - 4] = 0;

    byte program_mode;
    char program_mode_user_input = argv[1][1];
    switch (program_mode_user_input)
    {
    case 'e':
        program_mode = EXTRACT_MODE;
        strcat(output_file_name, "_extract.WAD");
        printf("Extraction mode enabled!\n");
        break;
    case 'd':
        program_mode = DECOMPRESS_MODE;
        strcat(output_file_name, "_decomp.WAD");
        printf("Decompression mode enabled!\n");
        break;
    case 'c':
        program_mode = COMPRESS_MODE;
        strcat(output_file_name, "_comp.WAD");
        printf("Compression mode enabled!\n");
        break;
    case 'p':
        program_mode = PAD_MODE;
        strcat(output_file_name, "_pad.WAD");
        printf("Padding mode enabled!\n");
        break;
    default:
        wadutil64_help();
        return EXIT_FAILURE;
    }

    // Create output file
    FILE* output_file = fopen(output_file_name, "wb");
    if (!output_file)
    {
        printf("ERROR: Could not write decompressed WAD!\n");
        return EXIT_FAILURE;
    }

    switch (program_mode)
    {
    case EXTRACT_MODE:
        extract_WAD(input_file, output_file);
        break;
    case DECOMPRESS_MODE:
        decompress_WAD(input_file, output_file);
        break;
    case COMPRESS_MODE:
        compress_WAD(input_file, output_file);
        break;
    case PAD_MODE:
        pad_WAD(input_file, output_file);
        break;
    
    default:
        break;
    }
    
    fclose(input_file);
    fclose(output_file);
    
    return EXIT_SUCCESS;
}