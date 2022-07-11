#include "doomdef.h"

int main(int argc, char **argv) {
    FILE *wad = fopen("DOOM64.WAD", "rb");

    wadinfo_t wad_header;

    fread(&wad_header, sizeof(wadinfo_t), 1, wad);

    printf("number of lumps: %d, address to directory: %X\n", wad_header.numlumps, wad_header.infotableofs);

    fseek(wad, wad_header.infotableofs, SEEK_SET);

    lumpinfo_t lump;
    fread(&lump, sizeof(lumpinfo_t), 1, wad);
    printf("data position in WAD: %d, size: %d, name: %s\n", lump.filepos, lump.size, lump.name);

    fread(&lump, sizeof(lumpinfo_t), 1, wad);
    lump.name[0] -= 0x80;
    printf("data position in WAD: %d, size: %d, name: %s\n", lump.filepos, lump.size, lump.name);

    fclose(wad);
    return EXIT_SUCCESS;
}