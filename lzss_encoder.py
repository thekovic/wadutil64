
def elements_in_array(check_elements, elements):
    i = 0
    offset = 0
    for element in elements:
        if len(check_elements) <= offset:
            # All of the elements in check_elements are in elements
            return i - len(check_elements)
        
        if check_elements[offset] == element:
            offset += 1
        else:
            offset = 0

        i += 1
    return -1

encoding = "utf-8"

def encode(text, max_sliding_window_size=4096):
    text_bytes = text.encode(encoding)

    search_buffer = [] # Array of integers, representing bytes
    check_characters = [] # Array of integers, representing bytes
    output = [] # Output array

    i = 0
    for char in text_bytes:
        index = elements_in_array(check_characters, search_buffer) # The index where the characters appears in our search buffer

        if elements_in_array(check_characters + [char], search_buffer) == -1 or i == len(text_bytes) - 1:
            if i == len(text_bytes) - 1 and elements_in_array(check_characters + [char], search_buffer) != -1:
                # Only if it's the last character then add the next character to the text the token is representing
                check_characters.append(char)
            
            if len(check_characters) > 1:
                index = elements_in_array(check_characters, search_buffer)
                offset = i - index - len(check_characters) # Calculate the relative offset
                length = len(check_characters) # Set the length of the token (how many character it represents)

                token = f"<{offset},{length}>" # Build our token

                if len(token) > length:
                    # Length of token is greater than the length it represents, so output the characters
                    output.extend(check_characters) # Output the characters
                else:
                    output.extend(token.encode(encoding)) # Output our token
                
                search_buffer.extend(check_characters) # Add the characters to our search buffer   
            else:
                output.extend(check_characters) # Output the character  
                search_buffer.extend(check_characters) # Add the characters to our search buffer   

            check_characters = []   
        
        check_characters.append(char)

        if len(search_buffer) > max_sliding_window_size: # Check to see if it exceeds the max_sliding_window_size
            search_buffer = search_buffer[1:] # Remove the first element from the search_buffer

        i += 1
    
    return bytes(output)

if __name__ == "__main__":
    print(encode("ABCDEF ABCDEF", 4096).decode(encoding))
    print(encode("supercalifragilisticexpialidocious supercalifragilisticexpialidocious", 1024).decode(encoding))
    print(encode("LZSS will take over the world!", 256).decode(encoding))
    print(encode("It even works with 😀s thanks to UTF-8", 16).decode(encoding))