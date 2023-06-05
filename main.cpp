#include "wadutil64_def.h"

typedef enum
{
    EXTRACT_MODE,
    DECOMPRESS_MODE,
    COMPRESS_MODE,
    PAD_MODE
} wadutil64_mode;

typedef enum
{
    DECODE_NONE,
    DECODE_JAGUAR,
    DECODE_D64
} decodetype;

typedef struct
{
    int         filepos;
    int         size;
    char        name[8];
} lumpinfo_t;

typedef struct
{
    char        identification[4];      /* should be IWAD */
    int         numlumps;
    int         infotableofs;
} wadinfo_t;

static char input_file_name[128];
static char output_file_name[128];

void choose_decode_mode(byte* decode_mode, char* lump_name)
{
    char MAP01_name[6] = "MAP01";
    
    if (!strcmp(lump_name, "S_START"))
    {
        *decode_mode = DECODE_JAGUAR;
    }
    else if (!strcmp(lump_name, "T_START"))
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

    MAP01_name[0] += 0x80;
    if (!strcmp(lump_name, MAP01_name))
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

lumpinfo_t* read_lump_directory(FILE* WAD, int number_of_lumps, int offset)
{
    // Allocate space in memory for lump directory into
    lumpinfo_t* lump_directory = (lumpinfo_t*) malloc(number_of_lumps * sizeof(lumpinfo_t));
    if (!lump_directory)
    {
        printf("ERROR: Could not read WAD lumps.");
        exit(EXIT_FAILURE);
    }

    // Read the lump directory
    fseek(WAD, offset, SEEK_SET);
    for (int i = 0; i < number_of_lumps; ++i)
    {
        fread(lump_directory + i, sizeof(lumpinfo_t), 1, WAD);
    }

    return lump_directory;
}

byte* read_lump(FILE* WAD, int offset, int size)
{
    byte* lump_data = (byte*) malloc(size);
    if (!lump_data)
    {
        printf("ERROR: Could not read WAD lump at %i of size %i.", offset, size);
        exit(EXIT_FAILURE);
    }

    fseek(WAD, offset, SEEK_SET);
    fread(lump_data, size, 1, WAD);

    return lump_data;
}

void extract_WAD(FILE* input_ROM, FILE* output_WAD)
{
    printf("TODO: WAD EXTRACTION!!!\n");
}

byte* decompress_lump_data(byte* lump_data, int new_size, byte decode_mode)
{
    byte* decompressed_lump = (byte*) malloc(new_size);
    if (!decompressed_lump)
    {
        printf("ERROR: Could not decompress WAD lump.");
        exit(EXIT_FAILURE);
    }

    if (decode_mode == DECODE_JAGUAR)
    {
        DecodeJaguar(lump_data, decompressed_lump);
    }
    else if (decode_mode == DECODE_D64)
    {
        DecodeD64(lump_data, decompressed_lump);
    }

    free(lump_data);
    return decompressed_lump;
}

void decompress_and_write_lump(FILE* input_WAD, FILE* output_WAD, lumpinfo_t* lump_info, int size, byte* decode_mode)
{
    choose_decode_mode(decode_mode, lump_info->name);
    // If empty marker lump, don't even bother and try to decompress
    if (size <= 0)
    {
        return;
    }

    byte* lump_data = read_lump(input_WAD, lump_info->filepos, size);

    if (lump_info->name[0] & 0x80)
    {
        lump_info->name[0] -= 0x80;
        char lump_name[9];
        strncpy(lump_name, lump_info->name, 8);
        lump_name[8] = 0;
        printf("Decompressing lump: %s\n", lump_name);
        lump_data = decompress_lump_data(lump_data, lump_info->size, *decode_mode);
    }

    fwrite(lump_data, lump_info->size, 1, output_WAD);
    free(lump_data);
}

void decompress_WAD(FILE* input_WAD, FILE* output_WAD)
{
    // Read WAD header
    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, input_WAD);
    printf("WAD name: %s\n", input_file_name);
    printf("Number of lumps: %d, Address to lump directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    // Read list of all lumps
    lumpinfo_t* lump_directory = read_lump_directory(input_WAD, wad_header.numlumps, wad_header.infotableofs);

    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);
    int total_size = sizeof(wadinfo_t);

    byte decode_mode = DECODE_NONE;

    // Process first lump
    {
        int lump_size = lump_directory[1].filepos - sizeof(wadinfo_t);
        decompress_and_write_lump(input_WAD, output_WAD, &(lump_directory[0]), lump_size, &decode_mode);

        lump_directory[0].filepos = sizeof(wadinfo_t);
        total_size += lump_directory[0].size;
    }
    
    // Process all other lumps except the last one
    for (int i = 1; i < wad_header.numlumps - 1; ++i)
    {
        int lump_size = lump_directory[i+1].filepos - lump_directory[i].filepos;
        decompress_and_write_lump(input_WAD, output_WAD, &(lump_directory[i]), lump_size, &decode_mode);

        lump_directory[i].filepos = lump_directory[i - 1].filepos + lump_directory[i - 1].size;
        total_size += lump_directory[i].size;
    }
    
    // Process last lump
    {
        int lump_size = wad_header.infotableofs - lump_directory[wad_header.numlumps - 1].filepos;
        decompress_and_write_lump(input_WAD, output_WAD, &(lump_directory[wad_header.numlumps - 1]), lump_size, &decode_mode);

        lump_directory[wad_header.numlumps - 1].filepos = lump_directory[wad_header.numlumps - 2].filepos + lump_directory[wad_header.numlumps - 2].size;
        total_size += lump_directory[wad_header.numlumps - 1].size;
    }

    // Write lump directory
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output_WAD);

    // Fix header using new size of WAD
    wad_header.infotableofs = total_size;
    fseek(output_WAD, 0, SEEK_SET);
    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);

    free(lump_directory);
}

int compress_and_write_lump(FILE* input_WAD, FILE* output_WAD, lumpinfo_t* lump_info, byte* decode_mode)
{
    choose_decode_mode(decode_mode, lump_info->name);
    // If empty marker lump, don't even bother and try to decompress
    if (lump_info->size <= 0)
    {
        return 0;
    }

    byte* lump_data = read_lump(input_WAD, lump_info->filepos, lump_info->size);
    bool can_free = true;
    int lump_size = lump_info->size;
    std::vector<byte> compressed_lump_data;
    
    if (*decode_mode == DECODE_JAGUAR)
    {
        // TODO: implement Jaguar Doom's compression (should be standard LZSS)
    }
    else if (*decode_mode == DECODE_D64)
    {
        char lump_name[9];
        strncpy(lump_name, lump_info->name, 8);
        lump_name[8] = 0;
        printf("Compressing lump: %s\n", lump_name);

        lump_info->name[0] += 0x80;
        compressed_lump_data = Deflate_Encode(lump_data, lump_info->size);
        lump_size = static_cast<int>(compressed_lump_data.size());
        lump_data = compressed_lump_data.data();
        can_free = false;
    }
    
    fwrite(lump_data, lump_size, 1, output_WAD);
    if (can_free)
    {
        free(lump_data);
    }

    return lump_size;
}

void compress_WAD(FILE* input_WAD, FILE* output_WAD)
{
    // Read WAD header
    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, input_WAD);
    printf("WAD name: %s\n", input_file_name);
    printf("Number of lumps: %d, Address to lump directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    // Read list of all lumps
    lumpinfo_t* lump_directory = read_lump_directory(input_WAD, wad_header.numlumps, wad_header.infotableofs);

    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);
    int total_size = sizeof(wadinfo_t);

    byte decode_mode = DECODE_NONE;

    // Process first lump
    {
        total_size += compress_and_write_lump(input_WAD, output_WAD, &(lump_directory[0]), &decode_mode);
        lump_directory[0].filepos = sizeof(wadinfo_t);
    }
    
    // Process all other lumps
    for (int i = 1; i < wad_header.numlumps; ++i)
    {
        int compressed_size = compress_and_write_lump(input_WAD, output_WAD, &(lump_directory[i]), &decode_mode);
        lump_directory[i].filepos = total_size;
        total_size += compressed_size;
    }

    // Write lump directory
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output_WAD);

    // Fix header using new size of WAD
    wad_header.infotableofs = total_size;
    fseek(output_WAD, 0, SEEK_SET);
    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);

    free(lump_directory);
}

byte* pad_lump(FILE* WAD, lumpinfo_t* lump_info)
{
    int mod = lump_info->size % 4;
    int padding = (mod != 0) ? 4 - mod : 0;

    byte* lump_data = read_lump(WAD, lump_info->filepos, lump_info->size);

    if (padding > 0)
    {
        // Calculate new size
        int padded_size = lump_info->size + padding;
        
        // Copy lump data with padding
        byte* padded_lump_data = (byte*) malloc(padded_size);
        memset(padded_lump_data, 0, padded_size);
        memcpy(padded_lump_data, lump_data, lump_info->size);

        // Fix entry in lump directory
        lump_info->size = padded_size;

        free(lump_data);
        lump_data = padded_lump_data;
    }

    return lump_data;
}

void pad_WAD(FILE* input_WAD, FILE* output_WAD)
{
    // Read WAD header
    wadinfo_t wad_header;
    fread(&wad_header, sizeof(wadinfo_t), 1, input_WAD);
    printf("WAD name: %s\n", input_file_name);
    printf("Number of lumps: %d, Address to lump directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    // Read list of all lumps
    lumpinfo_t* lump_directory = read_lump_directory(input_WAD, wad_header.numlumps, wad_header.infotableofs);

    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);
    int total_size = sizeof(wadinfo_t);

    // Pad first lump
    {
        byte* lump_data = pad_lump(input_WAD, &(lump_directory[0]));

        lump_directory[0].filepos = sizeof(wadinfo_t);
        total_size += lump_directory[0].size;

        fwrite(lump_data, lump_directory[0].size, 1, output_WAD);
        free(lump_data);
    }

    for (int i = 1; i < wad_header.numlumps; ++i)
    {
        byte* lump_data = pad_lump(input_WAD, &(lump_directory[i]));

        lump_directory[i].filepos = lump_directory[i - 1].filepos + lump_directory[i - 1].size;
        total_size += lump_directory[i].size;

        fwrite(lump_data, lump_directory[i].size, 1, output_WAD);
        free(lump_data);
    }

    // Write lump directory
    fwrite(lump_directory, sizeof(lumpinfo_t), wad_header.numlumps, output_WAD);

    // Fix header using new size of WAD
    wad_header.infotableofs = total_size;
    fseek(output_WAD, 0, SEEK_SET);
    fwrite(&wad_header, sizeof(wadinfo_t), 1, output_WAD);

    free(lump_directory);
}

int main(int argc, char** argv)
{
    std::ios::sync_with_stdio(false);

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
        printf("Extraction complete!\n");
        break;
    case DECOMPRESS_MODE:
        decompress_WAD(input_file, output_file);
        printf("Decompression complete!\n");
        break;
    case COMPRESS_MODE:
#if 0
        compress_WAD(input_file, output_file);
        printf("Compression complete!\n");
#endif
        printf("TODO: Compression not implemented yet.");
        break;
    case PAD_MODE:
        pad_WAD(input_file, output_file);
        printf("Padding complete!\n");
        break;
    
    default:
        break;
    }
    
    fclose(input_file);
    fclose(output_file);
    
    return EXIT_SUCCESS;
}