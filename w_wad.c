/* W_wad.c */

#include "doomdef.h"

/*============= */
/* GLOBALS */
/*============= */

static lumpcache_t  *lumpcache;             //800B2220
static int          numlumps;               //800B2224
static lumpinfo_t   *lumpinfo;              //800B2228 /* points directly to rom image */

static int          mapnumlumps;            //800B2230 psxdoom/doom64
static lumpinfo_t   *maplump;               //800B2234 psxdoom/doom64
static byte         *mapfileptr;            //800B2238 psxdoom/doom64


/*=========*/
/* EXTERNS */
/*=========*/

extern char _doom64_wadSegmentRomStart[], _doom64_wadSegmentRomEnd[];

/*
============================================================================

                        LUMP BASED ROUTINES

============================================================================
*/

/*
====================
=
= W_Init
=
====================
*/

void W_Init (void) // 8002BEC0
{
    wadinfo_t *wadfileptr;
    int infotableofs, i;

    wadfileptr = (wadinfo_t *)malloc(sizeof(wadinfo_t));

    printf("identification %s",wadfileptr->identification);

    if (strncasecmp(wadfileptr->identification, "IWAD", 4))
        printf("W_Init: invalid main IWAD id\n");

    numlumps = wadfileptr->numlumps;
    lumpinfo = (lumpinfo_t *) malloc(numlumps * sizeof(lumpinfo_t)); 

    infotableofs = wadfileptr->infotableofs;

	// TODO: read directory of lumps into lumpinfo

    printf("identification %s ",wadfileptr->identification);
    printf("numlumps %d ",numlumps);
    printf("infotableofs %d\n",infotableofs);

    for(i = 0; i < numlumps; i++)
    {
        printf("filepos %d        ",lumpinfo[i].filepos);
        printf("size %d           \n",lumpinfo[i].size);
    }

    lumpcache = (lumpcache_t *) malloc(numlumps * sizeof(lumpcache_t));
    memset(lumpcache, 0, numlumps * sizeof(lumpcache_t));
    free(wadfileptr);
}


/*
====================
=
= W_CheckNumForName
=
= Returns -1 if name not found
=
====================
*/
//int W_CheckNumForName(char *name, int unk1, int hibit1, int hibit2)    // original
int W_CheckNumForName(char *name, int hibit1, int hibit2) // 8002C0F4 removed unknown parameter
{
    char    name8[12];
    char    c, *tmp;
    int     i;
    lumpinfo_t  *lump_p;

    /* make the name into two integers for easy compares */

    *(int *)&name8[4] = 0;
    *(int *)&name8[0] = 0;

    tmp = name8;
    while ((c = *name) != 0)
    {
        *tmp++ = c;

        if ((tmp >= name8+8))
        {
            break;
        }
            

        name++;
    }

    /* scan backwards so patch lump files take precedence */

    lump_p = lumpinfo;
    for(i = 0; i < numlumps; i++)
    {
        if ((*(int *)&name8[0] == (*(int *)&lump_p->name[0] & hibit1)) &&
            (*(int *)&name8[4] == (*(int *)&lump_p->name[4] & hibit2)))
                return i;

        lump_p++;
    }

    return -1;
}

/*
====================
=
= W_GetNumForName
=
= Calls W_CheckNumForName, but bombs out if not found
=
====================
*/

int W_GetNumForName (char *name) // 8002C1B8
{
    int i;

    i = W_CheckNumForName (name, 0x7fffffff, 0xFFFFFFFF);
    if (i != -1)
        return i;

    printf ("W_GetNumForName: %s not found!",name);
    return -1;
}


/*
====================
=
= W_LumpLength
=
= Returns the buffer size needed to load the given lump
=
====================
*/

int W_LumpLength (int lump) // 8002C204
{
    if ((lump < 0) || (lump >= numlumps))
        printf ("W_LumpLength: lump %i out of range",lump);

    return lumpinfo[lump].size;
}


/*
====================
=
= W_ReadLump
=
= Loads the lump into the given buffer, which must be >= W_LumpLength()
=
====================
*/

void W_ReadLump (int lump, void *dest, decodetype dectype) // 8002C260
{
    byte *input;
    lumpinfo_t *l;
    int lumpsize;

    if ((lump < 0) || (lump >= numlumps))
        printf ("W_ReadLump: lump %i out of range",lump);

    l = &lumpinfo[lump];
    if(dectype != dec_none)
    {
        if ((l->name[0] & 0x80)) /* compressed */
        {
            lumpsize = l[1].filepos - (l->filepos);
            input = malloc(lumpsize);

            if (dectype == dec_jag)
                DecodeJaguar((byte *)input, (byte *)dest);
            else // dec_d64
                DecodeD64((byte *)input, (byte *)dest);

            free(input);
            return;
        }
    }

    if (l->name[0] & 0x80)
        lumpsize = l[1].filepos - (l->filepos);
    else
        lumpsize = (l->size);
}

/*
====================
=
= W_CacheLumpNum
=
====================
*/

void *W_CacheLumpNum (int lump, int tag, decodetype dectype) // 8002C430
{
    int lumpsize;
    lumpcache_t *lc;

    if ((lump < 0) || (lump >= numlumps))
        printf ("W_CacheLumpNum: lump %i out of range",lump);

    lc = &lumpcache[lump];

    if (!lc->cache)
    {   /* read the lump in */
        //if (dectype == dec_d64)
            //ST_DebugPrint("W_CacheLumpNum: lump %i", lump);

        if (dectype == dec_none)
            lumpsize = lumpinfo[lump + 1].filepos - lumpinfo[lump].filepos;
        else
            lumpsize = lumpinfo[lump].size;

        //malloc(lumpsize);

        W_ReadLump(lump, lc->cache, dectype);
    }
    else
    {
        if (tag & PU_CACHE) {
            //Z_Touch(lc->cache);
        }
    }

    return lc->cache;
}

/*
====================
=
= W_CacheLumpName
=
====================
*/

void *W_CacheLumpName (char *name, int tag, decodetype dectype) // 8002C57C
{
    return W_CacheLumpNum (W_GetNumForName(name), tag, dectype);
}


/*
============================================================================

MAP LUMP BASED ROUTINES

============================================================================
*/

/*
====================
=
= W_OpenMapWad
=
= Exclusive Psx Doom / Doom64
====================
*/

void W_OpenMapWad(int mapnum) // 8002C5B0
{
    int lump, size, infotableofs, i;
    char name [8];

    name[0] = 'M';
    name[1] = 'A';
    name[2] = 'P';
    name[3] = '0' + (char)(mapnum / 10);
    name[4] = '0' + (char)(mapnum % 10);
    name[5] = 0;

    lump = W_GetNumForName(name);
    size = W_LumpLength(lump);

    //printf("name %s           ",name);
    //printf("lump %d           ",lump);
    //printf("size %d           \n",size);

    mapfileptr = malloc(size);

    W_ReadLump(lump, mapfileptr, dec_d64);

    mapnumlumps = ((wadinfo_t*)mapfileptr)->numlumps;
    infotableofs = ((wadinfo_t*)mapfileptr)->infotableofs;

    //printf("mapnumlumps %d           ",mapnumlumps);
    //printf("infotableofs %d           \n",infotableofs);

    maplump = (lumpinfo_t*)(mapfileptr + infotableofs);

    for(i = 0; i < mapnumlumps; i++)
    {
        maplump[i].filepos = maplump[i].filepos;
        maplump[i].size = maplump[i].size;
    }
}

/*
====================
=
= W_FreeMapLump
=
= Exclusive Doom64
====================
*/

void W_FreeMapLump(void) // 8002C748
{
    free(mapfileptr);
    mapnumlumps = 0;
}

/*
====================
=
= W_MapLumpLength
=
= Exclusive Psx Doom / Doom64
====================
*/

int W_MapLumpLength(int lump) // 8002C77C
{
    if (lump >= mapnumlumps)
        printf("W_MapLumpLength: %i out of range", lump);

    return maplump[lump].size;
}


/*
====================
=
= W_MapGetNumForName
=
= Exclusive Psx Doom / Doom64
====================
*/

int W_MapGetNumForName(char *name) // 8002C7D0
{
    char    name8[12];
    char    c, *tmp;
    int     i;
    lumpinfo_t  *lump_p;

    /* make the name into two integers for easy compares */

    *(int *)&name8[4] = 0;
    *(int *)&name8[0] = 0;

    tmp = name8;
    while ((c = *name) != 0)
    {
        *tmp++ = c;

        if ((tmp >= name8+8))
            break;

        name++;
    }

    /* scan backwards so patch lump files take precedence */

    lump_p = maplump;
    for(i = 0; i < mapnumlumps; i++)
    {
        if ((*(int *)&name8[0] == (*(int *)&lump_p->name[0] & 0x7fffffff)) &&
            (*(int *)&name8[4] == (*(int *)&lump_p->name[4])))
                return i;

        lump_p++;
    }

    return -1;
}

/*
====================
=
= W_GetMapLump
=
= Exclusive Doom64
====================
*/

void  *W_GetMapLump(int lump) // 8002C890
{

    if (lump >= mapnumlumps)
        printf("W_GetMapLump: lump %d out of range", lump);

    return (void *) ((byte *)mapfileptr + maplump[lump].filepos);
}
