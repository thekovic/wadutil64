// Emacs style mode select   -*- C++ -*-
//-----------------------------------------------------------------------------
//
// $Id: DeflateN64.c 1251 2014-01-14 04:13:33Z svkaiser $
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
//
// $Author: svkaiser $
// $Revision: 1251 $
// $Date: 2014-01-13 22:13:33 -0600 (lun, 13 ene 2014) $
//
// DESCRIPTION: Deflate decompression in Doom64. Needs A LOT of cleanup and proper variable naming
//
//-----------------------------------------------------------------------------
#include <algorithm>
#include "wadutil64_def.h"

typedef struct {
    int var0;
    int var1;
    int var2;
    int var3;
    byte *write;
    byte *writePos;
    byte *read;
    byte *readPos;
    int null1;
    int null2;
} decoder_t;

static decoder_t decoder;

#define OVERFLOWCHECK       0x7FFFFFFF

#define TABLESIZE   1280
byte DecodeTable[TABLESIZE*4];

byte array01[0xFFFFFF];     // 0x800B3660
byte array05[0xFFFFFF];     // 0x8005D8A0
byte tableVar01[0xFFFFFF];  // 0x800B2250

int allocPtr[0xFFFFFF];

//**************************************************************
//**************************************************************
//  Deflate_InitDecodeTable
//**************************************************************
//**************************************************************

void Deflate_InitDecodeTable(void) {
    int v[2];
    int a[4];
    byte *a0p;
    byte *v1p;
    byte *a1p;
    byte *a2p;
    byte *a3p;
    byte *v0p;

    *(signed short*)array05 = 0x04;
    *(signed short*)(array05 + 2) = 0x06;
    *(signed short*)(array05 + 4) = 0x08;
    *(signed short*)(array05 + 6) = 0x0A;
    *(signed short*)(array05 + 8) = 0x0C;
    *(signed short*)(array05 + 10) = 0x0E;

    *(signed short*)(tableVar01+0x34) = 0x558F;

    *(int*)(tableVar01+0x3C) = 3;
    *(int*)(tableVar01+0x40) = 0;
    *(int*)(tableVar01+0x44) = 0;

    decoder.var0 = 0;
    decoder.var1 = 0;
    decoder.var2 = 0;
    decoder.var3 = 0;

    a0p = (array01 + 4);
    v1p = (byte*)(DecodeTable+0x9E4);

    v[0] = 2;

    do {
        if(v[0] < 0) {
            *(signed short*)v1p = (signed short)((v[0] + 1) << 1);
        }
        else {
            *(signed short*)v1p = (signed short)(v[0] >> 1);
        }

        *(signed short*)a0p = 1;

        v1p += 2;
        a0p += 2;

    }
    while(++v[0] < 1258);

    a1p = (byte*)(DecodeTable+0x4F2);
    a0p = (byte*)(DecodeTable+2);

    v[1] = 2;
    a[2] = 3;

    do {
        *(signed short*)a1p = a[2];
        a[2] += 2;

        *(signed short*)a0p = v[1];
        v[1] += 2;

        a0p += 2;
        a1p += 2;

    }
    while(a[2] < 1259);

    *(int*)tableVar01 = 0;
    v[1] = (1 << *(signed short*)(array05));
    *(int*)(tableVar01+0x18) = (v[1] - 1);

    *(int*)(tableVar01+4) = v[1];
    v[1] += (1 << *(signed short*)(array05 + 2));
    *(int*)(tableVar01+0x1C) = (v[1] - 1);

    v[0] = 2;
    a2p = (array05 + (v[0] << 1));

    a[0] = (v[0] << 2);
    a1p = (tableVar01 + a[0]);

    *(signed short*)a1p = v[1];

    v[1] += (1 << *(signed short*)a2p);
    *(int*)(a1p + 4) = v[1];

    v[1] += (1 << *(signed short*)(a2p + 2));
    *(int*)(a1p + 8) = v[1];

#ifdef _MSC_VER
    (int*)a3p = (int*)((byte*)(tableVar01+0x18) + a[0]);
#else
    a3p = ((byte*)(tableVar01+0x18) + a[0]);
#endif
    *(int*)a3p = (v[1] - 1);

    v[1] += (1 << *(signed short*)(a2p + 4));
    *(int*)(a1p + 12) = v[1];

    v[1] += (1 << *(signed short*)(a2p + 6));
    *(int*)(a3p + 4) = (v[1] - 1);
    *(int*)(a3p + 8) = (v[1] - 1);
    *(int*)(a3p + 0xc) = (v[1] - 1);

    v0p = (byte*)(tableVar01+0x30);

    *(int*)v0p = (v[1] - 1);
    *(int*)(v0p + 4) = ((v[1] - 1) + 64);
}

//**************************************************************
//**************************************************************
//  Deflate_GetDecodeByte
//**************************************************************
//**************************************************************

byte Deflate_GetDecodeByte(void) {
    if(!((decoder.readPos - decoder.read) < OVERFLOWCHECK)) {
        return -1;
    }

    return *decoder.readPos++;
}

//**************************************************************
//**************************************************************
//  Deflate_DecodeScan
//**************************************************************
//**************************************************************

int Deflate_DecodeScan(void) {
    int resultbyte;

    resultbyte = decoder.var0;

    decoder.var0 = (resultbyte - 1);
    if((resultbyte < 1)) {
        resultbyte = Deflate_GetDecodeByte();

        decoder.var1 = resultbyte;
        decoder.var0 = 7;
    }

    resultbyte = (0 < (decoder.var1 & 0x80));
    decoder.var1 = (decoder.var1 << 1);

    return resultbyte;
}

//**************************************************************
//**************************************************************
//  Deflate_CheckTable
//**************************************************************
//**************************************************************

void Deflate_CheckTable(int a0, int a1, int a2) {
    int i = 0;
    byte *t7p;
    byte *v0p;
    int idByte1;
    int idByte2;
    byte *tablePtr = (byte*)(DecodeTable+0x9E0);

    idByte1 = (a0 << 1);

    do {
        idByte2 = *(signed short*)(tablePtr + idByte1);

        t7p = (array01 + (idByte2 << 1));
        *(signed short*)t7p = (*(signed short*)(array01 + (a1 << 1)) + *(signed short*)(array01 + idByte1));

        a0 = idByte2;

        if(idByte2 != 1) {
            idByte1 = *(signed short*)(tablePtr + (idByte2 << 1));
            idByte2 = *(signed short*)(DecodeTable + (idByte1 << 1));

            a1 = idByte2;

            if(a0 == idByte2) {
                a1 = *(signed short*)((DecodeTable+0x4F0) + (idByte1 << 1));
            }
        }

        idByte1 = (a0 << 1);

    }
    while(a0 != 1);

    if(*(signed short*)(array01 + 2) != 0x7D0) {
        return;
    }

    *(signed short*)(array01 + 2) >>= 1;

    v0p = (byte*)(array01 + 4);

    do {
        *(signed short*)(v0p + 6) >>= 1;
        *(signed short*)(v0p + 4) >>= 1;
        *(signed short*)(v0p + 2) >>= 1;
        *(signed short*)(v0p) >>= 1;

        v0p += 8;
        i += 8;

    }
    while(i != 2512);
}

//**************************************************************
//**************************************************************
//  Deflate_DecodeByte
//**************************************************************
//**************************************************************

void Deflate_DecodeByte(int a0) {
    int v[2];
    int a[4];
    int s[10];
    byte *s4p;
    byte *s2p;
    byte *v1p;
    byte *s1p;
    byte *s6p;
    byte *s3p;
    byte *a1p;

    s4p = array01;
    v[0] = (a0 << 1);

    s2p = (byte*)(DecodeTable+0x9E0);

    v1p = (s4p + v[0]);
    s[5] = 1;

    a[2] = (a0 + 0x275);
    *(signed short*)(v1p + 0x4EA) = (*(signed short*)(v1p + 0x4EA) + 1);

    if(s[5] == *(signed short*)((s2p + v[0]) + 0x4EA)) {
        return;
    }

    v[1] = (a[2] << 1);

    s1p = (s2p + v[1]);

    s6p = (byte*)DecodeTable;

    a[3] = (*(signed short*)s1p << 1);
    a[1] = *(signed short*)(s6p + a[3]);
    s3p = (byte*)(DecodeTable+0x4F0);

    if(a[2] == a[1]) {
        a[1] = *(signed short*)(s3p + a[3]);
        a[0] = a[2];
        Deflate_CheckTable(a[0], a[1], a[2]);
        a[3] = (*(signed short*)s1p << 1);
    }
    else {
        a[0] = a[2];
        Deflate_CheckTable(a[0], a[1], a[2]);
        s3p = (byte*)(DecodeTable+0x4F0);
        a[3] = (*(signed short*)s1p << 1);
    }

    do {
        a[0] = (*(signed short*)(s2p + a[3]) << 1);

        a1p = (s6p + a[0]);
        v[0] = *(signed short*)a1p;
        s[0] = v[0];

        if(*(signed short*)s1p == v[0]) {
            s[0] = *(signed short*)(s3p + a[0]);
        }

        v1p = (s6p + a[3]);

        if(*(signed short*)(s4p + (s[0] << 1)) < *(signed short*)(s4p + v[1])) {
            if(*(signed short*)s1p == v[0]) {
                *(signed short*)(s3p + a[0]) = a[2];
            }
            else {
                *(signed short*)a1p = a[2];
            }

            a[1] = *(signed short*)v1p;
            if(a[2] == a[1]) {
                a[2] = *(signed short*)(s3p + a[3]);
                *(signed short*)v1p = s[0];
            }
            else {
                *(signed short*)(s3p + a[3]) = s[0];
                a[2] = a[1];
            }

            *(signed short*)(s2p + (s[0] << 1)) = *(signed short*)s1p;
            *(signed short*)s1p = *(signed short*)(s2p + a[3]);
            a[0] = s[0];
            a[1] = a[2];

            Deflate_CheckTable(a[0], a[1], a[2]);
            s1p = (s2p + (s[0] << 1));
        }

        a[2] = *(signed short*)s1p;
        v[1] = (a[2] << 1);

        s1p = (s2p + v[1]);
        a[3] = (*(signed short*)s1p << 1);

    }
    while(*(signed short*)s1p != s[5]);
}

//**************************************************************
//**************************************************************
//  Deflate_StartDecodeByte
//**************************************************************
//**************************************************************

int Deflate_StartDecodeByte(void) {
    int lookup = 1;                                 // $s0
    byte *tablePtr1 = DecodeTable;                  // $s2
    byte *tablePtr2 = (byte*)(DecodeTable+0x4F0);   // $s1

    while(lookup < 0x275) {
        if(Deflate_DecodeScan() == 0) {
            lookup = *(signed short*)(tablePtr1 + (lookup << 1));
        }
        else {
            lookup = *(signed short*)(tablePtr2 + (lookup << 1));
        }
    }

    lookup = (lookup + (signed short)0xFD8B);
    Deflate_DecodeByte(lookup);

    return lookup;
}

//**************************************************************
//**************************************************************
//  Deflate_RescanByte
//**************************************************************
//**************************************************************

int Deflate_RescanByte(int byte) {
    int i = 0;              // $s1
    int shift = 1;          // $s0
    int resultbyte = 0;     // $s2

    if(byte <= 0) {
        return resultbyte;
    }

    do {
        if(!(Deflate_DecodeScan() == 0)) {
            resultbyte |= shift;
        }

        i++;
        shift = (shift << 1);
    }
    while(i != byte);

    return resultbyte;
}

//**************************************************************
//**************************************************************
//  Deflate_WriteOutput
//**************************************************************
//**************************************************************

void Deflate_WriteOutput(byte outByte) {
    if(!((decoder.writePos - decoder.write) < OVERFLOWCHECK)) {
        printf("Overflowed output buffer");
        return;
    }

    *decoder.writePos++ = outByte;
}

//**************************************************************
//**************************************************************
//  Deflate_Decompress
//**************************************************************
//**************************************************************

void Deflate_Decompress(byte *input, byte *output) {
    int v[2];
    int a[4];
    int s[10];
    int t[10];
    int at;
    int div;
    int mul;
    int incrBit;

    byte *tablePtr1;
    byte *a1p;
    byte *s4p;
    byte *t8p;
    byte *t9p;
    byte *t1p;
    byte *v0p;
    byte *t2p;
    byte *t4p;

    Deflate_InitDecodeTable();
    incrBit = 0;

    decoder.read = input;
    decoder.readPos = input;

    decoder.null1 = 0x7FFFFFFF;

    decoder.write = output;
    decoder.writePos = output;

    tablePtr1 = (byte*)(tableVar01+0x34);

    decoder.null2 = 0x7FFFFFFF;

    a1p = tablePtr1;
    a[2] = 1;
    a[3] = 0;
    // Z_Alloc(a[0], a1p, a[2], a[3]);

    s4p = (byte*)allocPtr;

    v[0] = Deflate_StartDecodeByte();

    at = 256;
    s[0] = v[0];

    // GhostlyDeath <May 14, 2010> -- loc_8002E058 is part of a while loop
    while(v[0] != at) {
        at = (v[0] < 256);
        v[0] = 62;

        // GhostlyDeath <May 15, 2010> -- loc_8002E094 is an if statement
        if(at != 0) {
            a[0] = (s[0] & 0xff);
            Deflate_WriteOutput((byte)a[0]);

            t8p = s4p;
            t9p = (t8p + incrBit);
            *t9p = s[0];

            t[1] = *(int*)tablePtr1;

            incrBit += 1;
            if(incrBit == t[1]) {
                incrBit = 0;
            }
        }

        // GhostlyDeath <May 15, 2010> -- Since then old shots point to loc_8002E19C the remainder of
        // loc_8002E094 until loc_8002E19C is an else.
        else {
            t[2] = (s[0] + (signed short)0xFEFF);
            div = t[2] / v[0]; // (62)

            // GhostlyDeath <May 15, 2010> -- loc_8002E0AC is an adjacent jump (wastes cpu cycles for fun!)
            at = -1;

            // GhostlyDeath <May 15, 2010> -- loc_8002E0C4 is an if
            if(v[0] == at) {
                at = 0x8000;
            }

            s[2] = 0;
            s[5] = div;
            t[4] = (s[5] << 1);

            mul = s[5] * v[0];

            a[0] = *(signed short*)(array05 + t[4]);

            t[3] = mul;

            s[8] = (s[0] - t[3]);       // subu    $fp, $s0, $t3
            s[8] += (signed short)0xFF02;       // addiu   $fp, 0xFF02
            s[3] = s[8];                // move    $s3, $fp

            v[0] = Deflate_RescanByte(a[0]);

            t[5] = (s[5] << 2);
            t[6] = *(int*)(tableVar01 + t[5]);
            s[1] = incrBit;

            t[7] = (t[6] + v[0]);

            v[1] = (t[7] + s[8]);                   // addu $v1, $t7, $fp
            a[0] = (incrBit - v[1]);                // subu input, incrBit, $v1
            s[0] = a[0];                            // move $s0, input

            // GhostlyDeath <May 15, 2010> -- loc_8002E124 is an if
            if(a[0] < 0) {                      // bgez input, loc_8002E124
                t[8] = *(int*)tablePtr1;
                s[0] = (a[0] + t[8]);
            }

            // GhostlyDeath <May 15, 2010> -- loc_8002E184 is an if
            if(s[8] > 0)
                // GhostlyDeath <May 15, 2010> -- loc_8002E12C is a while loop (jump back from end)
                while(s[2] != s[3]) {
                    t9p = s4p;
                    t1p = (t9p + s[0]);
                    a[0] = *(byte*)t1p;             // lbu  input, 0($t1)
                    Deflate_WriteOutput((byte)a[0]);

                    v0p = s4p;
                    s[2] += 1;

                    t2p = (v0p + s[0]);                 // addu $t2, $s0, $v0
                    t[3] = *(byte*)t2p;

                    t4p = (v0p + s[1]);
                    *(byte*)t4p = t[3];

                    v[1] = *(int*)tablePtr1;

                    s[1]++;
                    s[0]++;

                    // GhostlyDeath <May 15, 2010> -- loc_8002E170 is an if
                    if(s[1] == v[1]) {
                        s[1] = 0;
                    }

                    // GhostlyDeath <May 15, 2010> -- loc_8002E17C is an if
                    if(s[0] == v[1]) {
                        s[0] = 0;
                    }
                }

            v[1] = *(int*)tablePtr1;
            incrBit += s[8];
            at = (incrBit < v[1]);

            // GhostlyDeath <May 15, 2010> -- loc_8002E19C is the end of a while
            if(at == 0) {
                incrBit -= v[1];
            }
        }

        v[0] = Deflate_StartDecodeByte();

        at = 256;
        s[0] = v[0];
    }

    a[1] = *(int*)s4p;
    // Z_Free();
}


// ---------------------------------------------------------------//
// Doom64 Encode Lump System Created by Erick Vásquez García [GEC]
// 12/08/2019
// ---------------------------------------------------------------//

std::vector<byte> BinCode;
std::vector<byte> OutFile(1024 * 1024 *4);
//FILE *out;
static int OutputSize = 0;
void MakeByte()
{
    int mult = 1;
    //for (int BinCnt = 0; BinCnt < BinCode.size();)
    
    //printf("Make File Output\n");
    
    while(1)
	{
        if(!BinCode.size()){break;}
        if(BinCode.size() < 8){break;}
        
        int tmpbyte = 8;
        int i = 0;              // $s1
        int shift = 1;          // $s0
        int resultbyte = 0;     // $s2
        int binary = 0;

        do
        {
            binary = *(BinCode.begin() + (7 - i));
            
            if(!(binary == 0)) {
                resultbyte |= shift;
            }
    
            i++;
            shift = (shift << 1);
            //printf("binary %x\n",binary);
        }
        while(i != tmpbyte);
        
        for(binary = 0; binary < i; binary++)//Remove first 8 bytes
        {
            BinCode.erase(BinCode.begin());
        }
        
        //printf("resultbyte shift %x\n",resultbyte);
        
        //fwrite (&resultbyte,sizeof(unsigned char), 1 ,out);
        byte* the_byte_result = (byte*) (&resultbyte);
        OutFile.push_back(*the_byte_result);
        OutputSize ++;
        //getch();
    }
}


//Count Table
int CountTable[64][6];

void InitCountTable()
{
     int i;
     
     for(i = 0; i <= 0x40; i++)
     {
           CountTable[i][0] = 0 + i;
           CountTable[i][1] = 16 + i;
           CountTable[i][2] = 80 + i;
           CountTable[i][3] = 336 + i;
           CountTable[i][4] = 1360 + i;
           CountTable[i][5] = 5456 + i;
     }
}


static byte BinaryTest[32] = {0};
void MakeBinary(int lookup, bool save)
{
     byte *tablePtr1 = DecodeTable;                  // $s2
     byte *tablePtr2 = (byte*)(DecodeTable+0x4F0);   // $s1
    
     int Code = lookup;
     int Cnt = 0;
     byte Binary[16] = {0};
     
     while (1)
     {
         if(lookup <= 1){break;}

         for(int i = 2; i < 1264; i += 2)
         {
              int lookupcheck = *(signed short*)(tablePtr1 + i);
              
              if(lookupcheck == lookup)
              {
                   //scan = 0;
                   //if(lookup == 0x0375) getch();
                   //printf("is 0 %x, poss %x, cnt %d\n", lookup, i, Cnt+1);
                   Binary[Cnt] = 0;
                   lookup = (i >> 1);
                   Cnt++;
                   break;
              }
         }
             
             
         for(int i = 2; i < 1264; i += 2)
         {
              int lookupcheck = *(signed short*)(tablePtr2 + i);
              
              if(lookupcheck == lookup)
              {
                   //scan = 1;
                   //if(lookup == 0x0375) getch();
                   //printf("is 1 %x, poss %x, cnt %d\n", lookup, i, Cnt+1);
                   Binary[Cnt] = 1;
                   lookup = (i >> 1);
                   Cnt++;
                   break;
              }
         }
     }
       
     if(save)
     {
         //Copy Binary
         for(int j = 0; j < Cnt; j++)
         {
              //printf("%d\n",Binary[(Cnt-1)-j]);
              BinCode.push_back(Binary[(Cnt-1)-j]);
              //BinCode.push_back(Binary[j]);
         }

         lookup = (Code + (signed short)0xFD8B);
         //printf("lookup %X\n",lookup);
         Deflate_DecodeByte(lookup);
     }
     else
     {
         //Copy Binary Test
         for(int j = 0; j < Cnt; j++)
         {
              BinaryTest[j] = Binary[(Cnt-1)-j];
         }
         
     }
     
     if(Code == 0x0375)
     {
         //getch();        
         int pow = (BinCode.size()) % 8;
         //printf("size %d pow4 %d\n",(BinCode.size()), pow);
         if(pow != 0)
         {
            //printf("Add\n");
            for (int i = 0 ; i < (8 - pow); i++)
            {
              BinCode.push_back(0x00);
            }
         }
     }
     
     //if(Cnt > 16)
        // getch();
}

void MakeExtraBinary(int Value, int Shift)
{
     unsigned char binario[16] = {0};
     unsigned short pixel = Value;

     int cnt = 0;
     //setcolor2(0x03);
     for(int b = 0; b < Shift; b++)
     {
          if((pixel & 1 << b) != 0)
          {
               //printf("1");
               BinCode.push_back(0x01);
               binario[b] = 1;
          }
          else
          {
               //printf("0");
               BinCode.push_back(0x00);//
               binario[b] = 0;
          }
          
          //if(b > 6 && b < 8) printf(" ");
     }
     
     //Copy Binary
     /*for(int j = 0; j < Shift; j++)
     {
          //printf("%d\n",Binary[(Cnt-1)-j]);
          BinCode.push_back(binario[j]);
          //BinCode.push_back(Binary[j]);
     }*/
     
     //printf(" %x\n",pixel);//getch();
     //setcolor2(0x07);
}

int last_prc = 0;

std::vector<byte> Deflate_Encode(byte *input, int size)
{
     int v[2];
     int a[4];
     int s[10];
     int t[10];
     int at;
     int div;
     int mul;
     
     int lookup = 1;                                 // $s0
     byte *tablePtr1 = DecodeTable;                  // $s2
     byte *tablePtr2 = (byte*)(DecodeTable+0x4F0);   // $s1
    
     byte *s4p;
     byte *t1p;
     byte *t2p;
     byte *t4p;
     byte *t8p;
     byte *t9p;
     byte *v0p;
     s4p = (byte*)allocPtr;
     int incrBit;
     int incrBitFile;
     int offset;
     bool copy;
     bool make;
     int i,j,k,l,m, n, bin;
     
     int Max = 0x558f;
     int LooKupCode = 0;
     
     InitCountTable();
     Deflate_InitDecodeTable();
     
     OutFile.clear();
     //out = fopen ("Compress.bin","wb");
     
     incrBitFile = 0;
     incrBit = 0;
     bin = 0;
     //Paso 1 Copy 14 Bytes
     
     for(i = 0; i < 14; i++)
     {
         t8p = s4p;
         t9p = (t8p + incrBit);
         *t9p = input[incrBitFile];
         
         //Make Binary
         LooKupCode = (input[incrBitFile] + 0x0275);
         MakeBinary(LooKupCode, true);
         MakeByte();
             
         incrBit++;
         incrBitFile++;
     }
     
     
     while(1)
     {
         if(incrBitFile > size) break;
         
         float orig_v = ((float)((incrBitFile))) /(size);
         float prc = std::clamp(orig_v, 0.0f, 1.0f);
         int prc_int = static_cast<int>(prc*100);
         if (prc_int % 10 == 0 && prc_int != last_prc)
         {
            last_prc = prc_int;
            printf("Compress (%d %)\n", prc_int);
         }
         //printf("Compress (%%%.2f)\n", prc*100);
         
         offset = 0;
         copy = false;
     
         //int pow2 = (BinCode.size()) % 8;
         //if(pow2 != 0)
         for(j = 64; j >= 3; j--)
         {
               if(copy) break;
               
               int minval = incrBit - 1024;
               for(k = incrBit; k >= minval; k--)
               {
                     if(copy) break;
                     for(l = 0; l < j; l++)
                     {
                           offset = (k - j) + l;
                           
                           if(offset < 0)
                           {
                                 //offset += Max;
                                 //continue;
                                 break;
                           }
            
                           //printf("offset %d poss = %d\n", offset, incrBitFile + l);
                           //printf("A = %X || B = %X\n", s4p[offset], input[incrBitFile + l]);
                           
                           if(s4p[offset] != input[incrBitFile + l])
                           {
                               copy = false;
                               break;
                           }
                           else
                           {
                               copy = true;
                           }
                           //getch();
                     }
               }
         }
         
         if(copy)
         {
               //printf("\nCopy\n");
               //printf("offset = %d || offset1 = %d || count %d\n", offset-j, incrBit, j+1);
               int rest = (incrBit - (offset-j));
               int count = (j+1);
               //printf("rest = %d\n", rest);
               
               //Make Count Code
               int ShiftVal[6] = {0x0f, 0x3F, 0xFF, 0x3FF, 0xFFF, 0x3FFF};
               int Shift = 0x04;
               for(m = 0; m < 6; m++)
               {
                     //printf("Count = %d -> %d ", count, CountTable[count][m]);
                     int maxval = CountTable[count][m] + ShiftVal[m];
                     //printf("Max %d  Shift %X", maxval, Shift);
                     if(rest <= maxval) {/*printf("\n");*/break; /*printf("This");*/}
                     Shift += 2;
               }
               //printf("\n"); 
               
               int ValExtra = (rest - CountTable[count][m]);
               //printf("ValExtra = %d\n", ValExtra);
               
               if(Shift == 0x04){LooKupCode = (0x0376 + (count - 3));}
               if(Shift == 0x06){LooKupCode = (0x03B4 + (count - 3));}
               if(Shift == 0x08){LooKupCode = (0x03F2 + (count - 3));}
               if(Shift == 0x0A){LooKupCode = (0x0430 + (count - 3));}
               if(Shift == 0x0C){LooKupCode = (0x046E + (count - 3));}
               if(Shift == 0x0E){LooKupCode = (0x04AC + (count - 3));}

               //printf("Code 0x%04X ValExtra %d\n", LooKupCode, ValExtra);
               
               //comprobando
               
               //BinaryTest
               MakeBinary(LooKupCode, false);
               
               bin = 0;
               lookup = 1;                                 // $s0
               while(lookup < 0x275) {
                    if(BinaryTest[bin] == 0) {
                        lookup = *(signed short*)(tablePtr1 + (lookup << 1));
                        //printf("lookup1 %X\n",lookup);
                    }
                    else {
                        lookup = *(signed short*)(tablePtr2 + (lookup << 1));
                        //printf("lookup2 %X\n",lookup);
                    }
                    bin++;
               }
               //printf("lookup %X\n",lookup);
               //getch();
               
               //lookup = (lookup + (signed short)0xFD8B);
               
               //s[0] = (LooKupCode + (signed short)0xFD8B);
               s[0] = (lookup + (signed short)0xFD8B);
               v[0] = 62;
               
               //s[0] = 256;
               t[2] = (s[0] + (signed short)0xFEFF);
               //printf("s[0] = %d\n",s[0]);
               //printf("t[2] = %d\n",t[2]);
               
               div = t[2] / v[0];
               
               at = -1;

               // GhostlyDeath <May 15, 2010> -- loc_8002E0C4 is an if
               if(v[0] == at) {
                     at = 0x8000;
               }

               s[2] = 0;
               s[5] = div;
               //printf("s[5] = %d\n",s[5]);
               t[4] = (s[5] << 1);
               //printf("t[4] = %d\n",t[4]);
    
               mul = s[5] * v[0];
    
               a[0] = *(signed short*)(array05 + t[4]);
               //printf("a[0] = %X t[4] = %X\n",a[0], t[4]);
    
               t[3] = mul;
               //printf("t[3] = %d\n",t[3]);
    
               s[8] = (s[0] - t[3]);       // subu    $fp, $s0, $t3
               //printf("s[8] = %d\n",s[8]);
               s[8] += (signed short)0xFF02;       // addiu   $fp, 0xFF02
               //printf("Count s[8] = %d\n",s[8]);
               s[3] = s[8];//Count to copy                // move    $s3, $fp
    
               //printf("shift a[0] = %d\n",a[0]);
               int shift = a[0];
               v[0] = ValExtra;//Deflate_RescanByte(a[0]);
               //printf("v[0] = %d\n",v[0]);
    
               t[5] = (s[5] << 2);
               //printf("t[5] = %d\n",t[5]);
               t[6] = *(int*)(tableVar01 + t[5]);
               //printf("t[6] = %d\n",t[6]);
               s[1] = incrBit;
    
               t[7] = (t[6] + v[0]);
               //printf("t[7] = %d\n",t[7]);
               v[1] = (t[7] + s[8]);                   // addu $v1, $t7, $fp
               //printf("Rest v[1] = %d\n",v[1]);//valor a restar
               int rest2 = v[1];
               a[0] = (incrBit - v[1]);                // subu input, incrBit, $v1
               //printf("incrBit %d, a[0] = %d\n",incrBit, a[0]);
               s[0] = a[0];                            // move $s0, input
    
               // GhostlyDeath <May 15, 2010> -- loc_8002E124 is an if
               if(a[0] < 0) {                      // bgez input, loc_8002E124
                    t[8] = Max;
                    s[0] = (a[0] + t[8]);
               }
                
               //printf("s[0] = %d\n",s[0]);
               
               // GhostlyDeath <May 15, 2010> -- loc_8002E184 is an if
               
            make = false;
            l = 0;
            //if(lookup == LooKupCode)
            if(s[8] > 0)
            {
            //printf("s[8] = %X\n",s[8]);
            // GhostlyDeath <May 15, 2010> -- loc_8002E12C is a while loop (jump back from end)
                while(s[2] != s[3])
                {
                    //printf("s[2] = %X\n",s[2]);
                    //printf("s[3] = %X\n",s[3]);
                    t9p = s4p;
                    t1p = (t9p + s[0]);
                    a[0] = *(byte*)t1p;             // lbu  input, 0($t1)
                    
                    if(a[0] == input[incrBitFile + l]) {make = true;}
                    else {make = false; break;}
                    //printf("a[0] = %X\n",a[0]);
                    //setcolor2(0x0B);//printf("%02X",a[0]);getch();
                    //printf("out a[0] = %X (%d , %d)\n",a[0],  s[0], incrBit+s[2]);//getch();

                    v0p = s4p;
                    s[2] += 1;

                    t2p = (v0p + s[0]);                 // addu $t2, $s0, $v0
                    t[3] = *(byte*)t2p;

                    t4p = (v0p + s[1]);
                    *(byte*)t4p = t[3];

                    v[1] = Max;
                    
                    //printf("s[0] = %d || s[1] = %d\n",s[0],s[1]);

                    s[1]++;
                    s[0]++;

                    // GhostlyDeath <May 15, 2010> -- loc_8002E170 is an if
                    if(s[1] == v[1]) {
                        s[1] = 0;
                    }

                    // GhostlyDeath <May 15, 2010> -- loc_8002E17C is an if
                    if(s[0] == v[1]) {
                        s[0] = 0;
                    }
                    
                    l++;
                }
             }
               
               if(make)
               {
                   MakeBinary(LooKupCode, true);
                   MakeByte();
                   MakeExtraBinary(ValExtra, Shift);
                   MakeByte();
                   
                   for(i = 0; i < (j+1); i++)
                   {
                         t8p = s4p;
                         t9p = (t8p + incrBit);
                         *t9p = input[incrBitFile];
                         incrBit++;
                         incrBitFile++;
                   }
    
                   at = (incrBit < Max);
                   if(at == 0)
                   {
                         incrBit -= Max;
                   }
                   //getch();
               }
               else
               {
                   //setcolor2(0x04);printf("No save\n");
                   t8p = s4p;
                   t9p = (t8p + incrBit);
                   *t9p = input[incrBitFile];
                     
                   //Make Binary
                   LooKupCode = (input[incrBitFile] + 0x0275);
                   //setcolor2(0x0A);printf("Code 0x%04X -> 0x%02X\n", LooKupCode, input[incrBitFile]);setcolor2(0x07);
                   MakeBinary(LooKupCode, true);
                   MakeByte();
                             
                   incrBit++;
                   incrBitFile++;
                   if(incrBit == Max)
                   {
                        incrBit = 0;
                   }
                   //getch();
               }
                     
               //getch();
         }
         else
         {
             t8p = s4p;
             t9p = (t8p + incrBit);
             *t9p = input[incrBitFile];
             
             //Make Binary
             LooKupCode = (input[incrBitFile] + 0x0275);
             //setcolor2(0x0A);printf("Code 0x%04X -> 0x%02X\n", LooKupCode, input[incrBitFile]);setcolor2(0x07);
             MakeBinary(LooKupCode, true);
             MakeByte();
                     
             incrBit++;
             incrBitFile++;
             if(incrBit == Max)
             {
                incrBit = 0;
             }
             //printf("\n"); 
             //getch();
         }
     }
     
     MakeBinary(0x0375, true);
     MakeByte();
     
     
     int Aling4 = OutputSize % 4;
     if(Aling4 != 0)
     {
        //printf("Add\n");
        for (i = 0 ; i < (4 - Aling4); i++)
        {
          //*output++ = 0;
          int val = 0x00;
          byte* the_byte_val = (byte*) (&val);
          //fwrite (&val, sizeof(unsigned char), 1 ,out);
          OutFile.push_back(*the_byte_val);
        }
     }
     
     //fclose(out);
     return OutFile;
    /* TEST
    FILE *f3 = fopen ("Alloc2.bin","wb");
    for(i = 0; i < Max; i++)
    {
    fwrite (&s4p[i],sizeof(byte),1,f3);
    }
    fclose(f3);
    */
}