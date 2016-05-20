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
    NSString *fontFilePath = [[NSBundle mainBundle] resourcePath];
    NSURL *fontsURL = [NSURL fileURLWithPath:fontFilePath];
    
    if (fontsURL != nil) {
        FSRef fsRef;
        CFURLGetFSRef((CFURLRef)fontsURL, &fsRef);
        
        OSStatus status = ATSFontActivateFromFileReference(&fsRef, kATSFontContextLocal, kATSFontFormatUnspecified, NULL, kATSOptionFlagsDefault, NULL);
        
        if (status != noErr) {
            NSLog(@"Unable to load fonts");
        } else {
            self.font = [NSFont fontWithName:@"Montserrat-Bold" size:12];
        }
    }    
}

@end