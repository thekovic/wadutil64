#include <cstdlib>
#include <iostream>
#include <cstdio>
#include <cstring>
#include <vector>

typedef unsigned char byte;

void DecodeD64(unsigned char *input, unsigned char *output);
void DecodeJaguar(unsigned char *input, unsigned char *output);
std::vector<byte> Deflate_Encode(byte *input, int size);