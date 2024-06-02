//
//  PutioTextField.m
//  put.io adder
//
//  Created by Nico on 01/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioTextField.h"

@implementation PutioTextField

- (void)awakeFromNib
{
    NSString *fontFilePath = [[NSBundle mainBundle] pathForResource:@"Montserrat-Bold" ofType:@"ttf"];
    NSURL *fontURL = [NSURL fileURLWithPath:fontFilePath];
    
    if (fontURL != nil) {
        CFErrorRef error;
        bool success = CTFontManagerRegisterFontsForURL((CFURLRef)fontURL, kCTFontManagerScopeProcess, &error);
        
        if (!success) {
            CFStringRef errorDescription = CFErrorCopyDescription(error);
            NSLog(@"Unable to load fonts: %@", errorDescription);
            CFRelease(errorDescription);
        } else {
            self.font = [NSFont fontWithName:@"Montserrat-Bold" size:12];
        }
    }
}

@end
